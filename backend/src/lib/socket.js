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
    const payload = jwt.verify(cookies.jwt, process.env.JWT_SECRET);
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
// Each account's chosen status. "offline" hides a connected account from everyone.
const chosenPresence = new Map();
function broadcast() {
  const connected = [...new Set([...io.sockets.sockets.values()].map((s) => s.userId))];
  const visible = connected.filter((id) => chosenPresence.get(id) !== "offline");
  io.emit("getOnlineUsers", visible);
  // Being in a voice room always reads as available, because people can still be called there.
  io.emit("getPresence", Object.fromEntries(visible.flatMap((id) => (voiceRoomUsers.has(id) ? [[id, "room"]] : ["away", "busy"].includes(chosenPresence.get(id)) ? [[id, chosenPresence.get(id)]] : []))));
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
  socket.on("disconnect", () => {
    broadcast();
  });
});
export { io, app, server };
