// Transient signaling state only. No audio or call history is stored.
import { directCallUsers, voiceRoomUsers } from "./voiceActivity.js";
export function installCalls(io) {
  const calls = new Map(),
    active = directCallUsers;
  const finish = (call, reason) => {
    clearTimeout(call.timer);
    calls.delete(call.id);
    active.delete(call.caller);
    active.delete(call.callee);
    io.to(call.callerSocket)
      .to(call.calleeSocket || call.callee)
      .emit("call:ended", { callId: call.id, reason });
  };
  return (socket) => {
    let count = 0;
    const timer = setInterval(() => {
      count = 0;
    }, 10000);
    socket.on("call:signal", ({ to, signal } = {}, ack = () => {}) => {
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
        if (
          calls.has(signal.callId) ||
          active.has(socket.userId) ||
          active.has(to) ||
          voiceRoomUsers.has(socket.userId) ||
          voiceRoomUsers.has(to)
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
        active.set(call.callee, call.id);
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
        call.calleeSocket = socket.id;
        clearTimeout(call.timer);
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
