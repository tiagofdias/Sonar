import "dotenv/config";
import { Server } from "socket.io";
import http from "http";
import express from "express";
import jwt from "jsonwebtoken";
import User from "../models/user.model.js";
import { validSession, sessionHash } from "./sessions.js";
import { installCalls } from "./calls.js";
import { installVoiceRooms } from "./voiceRooms.js";
import { voiceRoomUsers } from "./voiceActivity.js";
import { watchSocketSession } from "./socketSession.js";
import { areFriends, friendIds } from "./friends.js";
import { connection } from "./push.js";
import Community from "../models/community.model.js";
const app = express();
const server = http.createServer(app);
export const allowedOrigins =
  process.env.NODE_ENV === "production"
    ? [process.env.FRONTEND_URL].filter(Boolean)
    : [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "http://localhost:3000",
      ];
const io = new Server(server, {
  cors: { origin: allowedOrigins, credentials: true },
  maxHttpBufferSize: 64000,
  allowRequest: (req, done) =>
    done(
      null,
      !req.headers.origin ||
        allowedOrigins.includes(req.headers.origin) ||
        (process.env.NODE_ENV === "production" &&
          req.headers.origin === `https://${req.headers.host}`),
    ),
});
io.use(async (socket, next) => {
  try {
    const cookies = Object.fromEntries(
      (socket.handshake.headers.cookie || "")
        .split(";")
        .map((c) => c.trim().split("=")),
    );
    const payload = jwt.verify(cookies.jwt, process.env.JWT_SECRET, { algorithms: ["HS256"] });
    if (!Number.isFinite(payload.exp)) throw new Error();
    const user = await User.findById(payload.userId).select("sessionVersion presence");
    if (!user || (payload.sessionVersion || 0) !== (user.sessionVersion || 0) || !(await validSession(payload)))
      throw new Error();
    socket.userId = String(payload.userId);
    socket.presence = user.presence || "online";
    socket.sessionVersion = payload.sessionVersion || 0;
    socket.sessionId = payload.sessionId;
    socket.tokenExpiresAt = payload.exp;
    socket.sessionHash = payload.sessionId ? sessionHash(payload.sessionId) : null;
    next();
  } catch {
    next(new Error("Authentication required"));
  }
});
connection.isOnline = (id) => (io.sockets.adapter.rooms.get(String(id))?.size || 0) > 0;
const objectId = (value) => typeof value === "string" && /^[a-f0-9]{24}$/i.test(value);
// Each account's chosen status. "offline" hides a connected account from everyone.
const chosenPresence = new Map();
// Who may learn that someone is online or what animal they chose: only their
// friends and the people they share a community with, never every account.
const AUDIENCE_MS = 20000;
const audiences = new Map();
export async function audienceOf(userId) {
  const id = String(userId);
  const cached = audiences.get(id);
  if (cached && Date.now() - cached.at < AUDIENCE_MS) return cached.ids;
  const [friends, communities] = await Promise.all([
    friendIds(id),
    Community.find({ members: id }).select("members").lean(),
  ]);
  const ids = new Set([id, ...friends.map(String)]);
  for (const community of communities) for (const member of community.members) ids.add(String(member));
  if (audiences.size > 20000) audiences.clear();
  audiences.set(id, { ids, at: Date.now() });
  return ids;
}
export async function emitToAudience(userId, event, data) {
  try {
    for (const id of await audienceOf(userId)) io.to(id).emit(event, data);
  } catch { /* A courtesy update; the next refresh carries it. */ }
}
async function sendPresence() {
  const connected = [...new Set([...io.sockets.sockets.values()].map((s) => s.userId))];
  const visible = connected.filter((id) => chosenPresence.get(id) !== "offline");
  // A status someone picked always shows. Otherwise being in a voice room reads as available.
  const status = (id) => (["away", "busy"].includes(chosenPresence.get(id)) ? chosenPresence.get(id) : voiceRoomUsers.has(id) ? "room" : null);
  await Promise.all(connected.map(async (viewer) => {
    try {
      const audience = await audienceOf(viewer);
      const seen = visible.filter((id) => audience.has(id));
      io.to(viewer).emit("getOnlineUsers", seen);
      io.to(viewer).emit("getPresence", Object.fromEntries(seen.flatMap((id) => (status(id) ? [[id, status(id)]] : []))));
    } catch { /* The next change sends it again. */ }
  }));
}
// Several changes at once (a reconnect storm) are sent as one update.
let presenceQueued = false;
function broadcast() {
  if (presenceQueued) return;
  presenceQueued = true;
  setTimeout(() => { presenceQueued = false; sendPresence(); }, 50);
}
// Call when friendships or memberships change so the people involved see each other at once.
export function presenceAudienceChanged(...ids) {
  for (const id of ids) audiences.delete(String(id));
  broadcast();
}
export function setPresence(userId, presence) {
  chosenPresence.set(String(userId), presence);
  broadcast();
  io.to(String(userId)).emit("ownPresence", { presence });
}
const connectCalls = installCalls(io);
const connectVoiceRooms = installVoiceRooms(io, broadcast);
io.on("connection", (socket) => {
  socket.join(socket.userId);
  if (socket.sessionHash) socket.join(`session:${socket.sessionHash}`);
  const verify = async () => {
    if (!Number.isFinite(socket.tokenExpiresAt) || Date.now() >= socket.tokenExpiresAt * 1000) return false;
    const user = await User.findById(socket.userId).select("sessionVersion");
    return Boolean(user && (user.sessionVersion || 0) === socket.sessionVersion && await validSession({ userId: socket.userId, sessionVersion: socket.sessionVersion, sessionId: socket.sessionId }));
  };
  watchSocketSession(socket, verify);
  socket.use(async (_packet, next) => {
    try {
      if (!(await verify())) {
        socket.emit("sessionRevoked");
        socket.disconnect(true);
        return;
      }
      next();
    } catch {
      next(new Error("Session could not be verified"));
    }
  });
  chosenPresence.set(socket.userId, socket.presence);
  broadcast();
  connectCalls(socket);
  connectVoiceRooms(socket);
  // Who is typing is passed along and never stored.
  let lastTyping = 0;
  socket.on("typing", async (data) => {
    const now = Date.now();
    if (now - lastTyping < 1500) return;
    lastTyping = now;
    try {
      if (objectId(data?.to)) {
        if (await areFriends(socket.userId, data.to)) io.to(data.to).emit("typing", { from: socket.userId });
      } else if (objectId(data?.communityId) && objectId(data?.channelId)) {
        const community = await Community.findOne({ _id: data.communityId, members: socket.userId, "channels._id": data.channelId }).select("members").lean();
        for (const member of community?.members || [])
          if (String(member) !== socket.userId) io.to(String(member)).emit("typing", { from: socket.userId, communityId: data.communityId, channelId: data.channelId });
      }
    } catch { /* Typing is only a courtesy. */ }
  });
  socket.on("disconnect", () => {
    broadcast();
  });
});
export { io, app, server };
