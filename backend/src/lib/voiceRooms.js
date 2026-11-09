import { validScreenSignal } from "./calls.js";
import { randomUUID } from "node:crypto";
import Community from "../models/community.model.js";
import User from "../models/user.model.js";
import { directCallUsers, voiceRoomUsers } from "./voiceActivity.js";
const rooms = new Map(),
  revisions = new Map();
let transport;
const keyFor = (communityId, channelId) => `${communityId}:${channelId}`;
const validId = (value) =>
  typeof value === "string" && /^[a-f0-9]{24}$/i.test(value);
function snapshot(room) {
  return {
    communityId: room.communityId,
    channelId: room.channelId,
    participants: [...room.participants.values()].map(
      ({ socketId, ...participant }) => participant,
    ),
    limit: 6,
  };
}
function broadcast(room) {
  for (const member of room.members)
    transport.to(String(member)).emit("voice:room-state", snapshot(room));
}
function leave(userId, reason = "left") {
  const active = voiceRoomUsers.get(userId);
  if (!active) return;
  const room = rooms.get(active.key),
    participant = room?.participants.get(userId);
  voiceRoomUsers.delete(userId);
  roomsChanged();
  if (!room || !participant) return;
  room.participants.delete(userId);
  transport
    .to(participant.socketId)
    .emit("voice:room-left", {
      communityId: room.communityId,
      channelId: room.channelId,
      sessionId: participant.sessionId,
      reason,
    });
  broadcast(room);
  if (!room.participants.size) rooms.delete(active.key);
}
export function reconcileVoiceRooms(community) {
  if (!transport) return;
  const communityId = String(community._id);
  revisions.set(communityId, community.revision);
  for (const room of [...rooms.values()])
    if (room.communityId === communityId) {
      room.members = community.members.map(String);
      const channelExists = community.channels.some(channel => String(channel._id) === room.channelId && channel.type === "voice");
      for (const userId of [...room.participants.keys()])
        if (!channelExists || !room.members.includes(userId)) leave(userId, channelExists ? "removed" : "channelDeleted");
    }
}
let roomsChanged = () => {};
export function installVoiceRooms(io, onChange = () => {}) {
  roomsChanged = onChange;
  transport = io;
  return (socket) => {
    let count = 0;
    const timer = setInterval(() => {
      count = 0;
    }, 10000);
    const handle = (event, action) =>
      socket.on(event, async (payload = {}, ack) => {
        const reply = (value) => {
          if (typeof ack === "function") ack(value);
        };
        if (++count > 500)
          return reply({ error: "Too many voice-room requests" });
        try {
          await action(payload, reply);
        } catch {
          reply({ error: "Voice room request failed. Try again." });
        }
      });
    async function authorize(communityId, channelId) {
      if (!validId(communityId) || (channelId && !validId(channelId)))
        return null;
      let community = await Community.findOne({
        _id: communityId,
        members: socket.userId,
      });
      if (community && community.revision < (revisions.get(communityId) || 0))
        community = await Community.findOne({
          _id: communityId,
          members: socket.userId,
        });
      if (!community || community.revision < (revisions.get(communityId) || 0))
        return null;
      if (channelId && community.channels.id(channelId)?.type !== "voice")
        return null;
      return community;
    }
    handle("voice:rooms", async ({ communityId }, reply) => {
      const community = await authorize(communityId);
      if (!community) return reply({ error: "Community unavailable" });
      reply({
        rooms: [...rooms.values()]
          .filter((room) => room.communityId === communityId)
          .map(snapshot),
      });
    });
    handle("voice:room-join", async ({ communityId, channelId }, reply) => {
      const community = await authorize(communityId, channelId);
      if (!community) return reply({ error: "Voice room unavailable" });
      if (!socket.connected) return;
      if (directCallUsers.has(socket.userId))
        return reply({
          error: "End your direct call before joining a voice room",
        });
      const current = voiceRoomUsers.get(socket.userId),
        key = keyFor(communityId, channelId);
      if (current) {
        if (current.key === key && current.socketId === socket.id)
          return reply({
            ...snapshot(rooms.get(key)),
            sessionId: current.sessionId,
          });
        return reply({ error: "Leave your current voice room first" });
      }
      if (community.channels.id(channelId)?.locked)
        return reply({ error: "This voice room is locked to new participants." });
      let room = rooms.get(key);
      if (room?.participants.size >= 6)
        return reply({ error: "This voice room is full (6 people maximum)" });
      const user = await User.findById(socket.userId).select("fullName");
      if (
        !user ||
        !socket.connected ||
        voiceRoomUsers.has(socket.userId) ||
        directCallUsers.has(socket.userId)
      )
        return reply({ error: "Voice state changed. Try again." });
      if (community.revision < (revisions.get(communityId) || 0))
        return reply({ error: "Membership changed. Try joining again." });
      room = rooms.get(key);
      if (room?.participants.size >= 6)
        return reply({ error: "This voice room is full (6 people maximum)" });
      if (!room) {
        room = {
          communityId,
          channelId,
          members: community.members.map(String),
          participants: new Map(),
        };
        rooms.set(key, room);
      }
      room.members = community.members.map(String);
      const participant = {
        userId: socket.userId,
        name: user.fullName,
        sessionId: randomUUID(),
        socketId: socket.id,
        muted: false,
        deafened: false,
      };
      room.participants.set(socket.userId, participant);
      voiceRoomUsers.set(socket.userId, {
        key,
        socketId: socket.id,
        sessionId: participant.sessionId,
      });
      roomsChanged();
      broadcast(room);
      reply({ ...snapshot(room), sessionId: participant.sessionId });
    });
    handle("voice:room-kick", async ({ communityId, channelId, to, toSession }, reply) => {
      if (!validId(to) || typeof toSession !== "string" || to === socket.userId)
        return reply({ error: "Participant unavailable" });
      const community = await authorize(communityId, channelId);
      if (!community) return reply({ error: "Voice room unavailable" });
      const owner = String(community.ownerId) === socket.userId;
      const moderators = community.moderators.map(String);
      if ((!owner && !moderators.includes(socket.userId)) || to === String(community.ownerId) || (!owner && moderators.includes(to)))
        return reply({ error: "You cannot disconnect this participant" });
      const room = rooms.get(keyFor(communityId, channelId));
      const participant = room?.participants.get(to);
      if (!participant || participant.sessionId !== toSession || community.revision < (revisions.get(communityId) || 0))
        return reply({ error: "Participant or permissions changed" });
      leave(to, "kicked");
      reply({ ok: true });
    });
    handle("voice:room-leave", async ({ sessionId }, reply) => {
      const active = voiceRoomUsers.get(socket.userId);
      if (!active || active.socketId !== socket.id || typeof sessionId !== "string" || active.sessionId !== sessionId)
        return reply({ error: "Voice room session changed" });
      leave(socket.userId);
      reply({ ok: true });
    });
    handle("voice:room-status", async ({ sessionId, muted, deafened }, reply) => {
      const active = voiceRoomUsers.get(socket.userId),
        room = rooms.get(active?.key);
      if (
        !room ||
        active.socketId !== socket.id ||
        active.sessionId !== sessionId ||
        typeof muted !== "boolean" ||
        typeof deafened !== "boolean"
      )
        return reply({ error: "Voice room unavailable" });
      if (!(await authorize(room.communityId, room.channelId))) {
        if (voiceRoomUsers.get(socket.userId)?.sessionId === sessionId) leave(socket.userId, "removed");
        return reply({ error: "Voice room unavailable" });
      }
      if (voiceRoomUsers.get(socket.userId)?.sessionId !== sessionId || room.participants.get(socket.userId)?.sessionId !== sessionId)
        return reply({ error: "Voice room session changed" });
      Object.assign(room.participants.get(socket.userId), { muted, deafened });
      broadcast(room);
      reply({ ok: true });
    });
    handle(
      "voice:room-signal",
      async (
        { communityId, channelId, sessionId, to, toSession, signal },
        reply,
      ) => {
        const active = voiceRoomUsers.get(socket.userId),
          room = rooms.get(active?.key);
        if (
          !room ||
          active.socketId !== socket.id ||
          active.sessionId !== sessionId ||
          room.communityId !== communityId ||
          room.channelId !== channelId ||
          to === socket.userId ||
          !signal ||
          JSON.stringify(signal).length > 32000
        )
          return reply({ error: "Voice session unavailable" });
        if (!(await authorize(communityId, channelId))) {
          if (voiceRoomUsers.get(socket.userId)?.sessionId === sessionId) leave(socket.userId, "removed");
          return reply({ error: "Voice room unavailable" });
        }
        const peer = room.participants.get(to);
        if (
          !peer ||
          peer.sessionId !== toSession ||
          voiceRoomUsers.get(socket.userId)?.sessionId !== sessionId
        )
          return reply({ error: "Participant left the room" });
        const valid = signal.type?.startsWith?.("screen-") ? validScreenSignal(signal) :
          signal.type === "ice"
            ? typeof signal.candidate?.candidate === "string"
            : ["offer", "answer"].includes(signal.type) &&
              signal.sdp?.type === signal.type &&
              typeof signal.sdp.sdp === "string";
        if (!valid) return reply({ error: "Invalid room signal" });
        io.to(peer.socketId).emit("voice:room-signal", {
          communityId,
          channelId,
          from: socket.userId,
          sessionId,
          toSession,
          signal,
        });
        reply({ ok: true });
      },
    );
    socket.on("disconnect", () => {
      clearInterval(timer);
      if (voiceRoomUsers.get(socket.userId)?.socketId === socket.id)
        leave(socket.userId, "disconnected");
    });
  };
}
