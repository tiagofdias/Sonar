// Transient signaling state only. No audio or call history is stored.
import { directCallUsers, voiceRoomUsers, answeredCalls, activity } from "./voiceActivity.js";
import { areFriends } from "./friends.js";
import { pushTo } from "./push.js";
export function validScreenSignal(signal) {
  if (signal.type === "screen-stop") return true;
  if (signal.type === "screen-ice")
    return ["sharer", "viewer"].includes(signal.role) && typeof signal.candidate?.candidate === "string";
  const kind = { "screen-offer": "offer", "screen-answer": "answer" }[signal.type];
  return Boolean(kind) && signal.sdp?.type === kind && typeof signal.sdp.sdp === "string";
}
export function installCalls(io) {
  const calls = new Map(),
    active = directCallUsers,
    // A second call ringing for someone already talking: callee -> call id.
    waiting = new Map();
  const release = (user, call) => {
    if (active.get(user) === call.id) active.delete(user);
    if (waiting.get(user) === call.id) waiting.delete(user);
  };
  const finish = (call, reason) => {
    clearTimeout(call.timer);
    calls.delete(call.id);
    release(call.caller, call);
    release(call.callee, call);
    if (answeredCalls.delete(call.id)) activity.changed();
    io.to(call.callerSocket)
      .to(call.calleeSocket || call.callee)
      .emit("call:ended", { callId: call.id, reason });
  };
  return (socket) => {
    let count = 0;
    const timer = setInterval(() => {
      count = 0;
    }, 10000);
    socket.on("call:signal", async ({ to, signal } = {}, ack = () => {}) => {
      const reply = (data) => {
        if (typeof ack === "function") ack(data);
      };
      if (
        ++count > 200 ||
        typeof to !== "string" ||
        !/^[a-f0-9]{24}$/i.test(to) ||
        !signal ||
        typeof signal.callId !== "string" ||
        !/^[a-f0-9-]{36}$/i.test(signal.callId) ||
        JSON.stringify(signal).length > 32000
      )
        return reply({ error: "Invalid call signal" });
      if (signal.type === "offer") {
        if (
          signal.sdp?.type !== "offer" ||
          typeof signal.sdp.sdp !== "string" ||
          to === socket.userId
        )
          return reply({ error: "Invalid call offer" });
        const friends = await areFriends(socket.userId, to).catch(() => false);
        if (!friends) return reply({ error: "You can only call your friends" });
        pushTo(to, "call", { sender: socket.userId });
        // Someone in an answered call or a voice room can be offered one call to swap to.
        const canWait = (Boolean(calls.get(active.get(to))?.calleeSocket) || (voiceRoomUsers.has(to) && !active.has(to))) && !waiting.has(to);
        if (
          calls.has(signal.callId) ||
          active.has(socket.userId) ||
          (active.has(to) && !canWait) ||
          voiceRoomUsers.has(socket.userId)
        )
          return reply({ error: "This person is already in a call" });
        if (
          ![...io.sockets.sockets.values()].some((peer) => peer.userId === to)
        )
          return reply({ error: "This person is offline" });
        const call = {
          id: signal.callId,
          caller: socket.userId,
          callee: to,
          callerSocket: socket.id,
          calleeSocket: null,
        };
        calls.set(call.id, call);
        active.set(call.caller, call.id);
        if (active.has(call.callee) || voiceRoomUsers.has(call.callee)) waiting.set(call.callee, call.id);
        else active.set(call.callee, call.id);
        call.timer = setTimeout(() => finish(call, "unanswered"), 60000);
        io.to(to).emit("call:signal", { from: socket.userId, signal });
        reply({ ok: true });
        return;
      }
      const call = calls.get(signal.callId);
      if (!call) return reply({ error: "Call is no longer active" });
      const caller =
        socket.userId === call.caller &&
        socket.id === call.callerSocket &&
        to === call.callee;
      const callee =
        socket.userId === call.callee &&
        (!call.calleeSocket || socket.id === call.calleeSocket) &&
        to === call.caller;
      if (!caller && !callee) return reply({ error: "Call is unavailable" });
      // Screen sharing runs on its own peer connection, inside an answered call.
      if (signal.type?.startsWith?.("screen-")) {
        if (!call.calleeSocket || !validScreenSignal(signal))
          return reply({ error: "Invalid screen signal" });
        io.to(caller ? call.calleeSocket : call.callerSocket).emit("call:signal", {
          from: socket.userId,
          signal,
        });
        reply({ ok: true });
        return;
      }
      if (signal.type === "hangup") {
        finish(call, "ended");
        reply({ ok: true });
        return;
      }
      if (signal.type === "answer") {
        if (
          !callee ||
          call.calleeSocket ||
          signal.sdp?.type !== "answer" ||
          typeof signal.sdp.sdp !== "string"
        )
          return reply({ error: "Invalid call answer" });
        // Answering a waiting call ends the one it replaces.
        const replaced = calls.get(active.get(call.callee));
        if (replaced && replaced !== call) finish(replaced, "ended");
        waiting.delete(call.callee);
        active.set(call.callee, call.id);
        call.calleeSocket = socket.id;
        clearTimeout(call.timer);
        answeredCalls.set(call.id, [String(call.caller), String(call.callee)]);
        activity.changed();
        io.to(call.callee)
          .except(socket.id)
          .emit("call:ended", { callId: call.id, reason: "answeredElsewhere" });
        io.to(call.callerSocket).emit("call:signal", {
          from: socket.userId,
          signal,
        });
        reply({ ok: true });
        return;
      }
      if (
        signal.type === "ice" &&
        signal.candidate &&
        typeof signal.candidate.candidate === "string"
      ) {
        io.to(
          caller ? call.calleeSocket || call.callee : call.callerSocket,
        ).emit("call:signal", { from: socket.userId, signal });
        reply({ ok: true });
        return;
      }
      reply({ error: "Invalid call signal" });
    });
    socket.on("disconnect", () => {
      clearInterval(timer);
      const held = calls.get(waiting.get(socket.userId));
      if (held && ![...io.sockets.sockets.values()].some((peer) => peer.userId === socket.userId))
        finish(held, "peerDisconnected");
      const call = calls.get(active.get(socket.userId));
      if (
        call &&
        (call.callerSocket === socket.id ||
          call.calleeSocket === socket.id ||
          ![...io.sockets.sockets.values()].some(
            (peer) => peer.userId === socket.userId,
          ))
      )
        finish(call, "peerDisconnected");
    });
  };
}
