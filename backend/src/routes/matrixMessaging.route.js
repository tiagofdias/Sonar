// Experimental encrypted room transport. Callers cannot choose arbitrary room
// membership or supply their own identity.
import express from "express";
import { protectRoute } from "../middleware/auth.middleware.js";
import { createAuthRateLimit } from "../middleware/authRateLimit.js";
import { querySessionRoomDeviceKeys, claimSessionRoomOneTimeKey } from "../lib/matrixKeys.js";
import { enqueueSessionRoomKeyShares, readSessionRoomToDevice, acknowledgeSessionRoomToDevice, enqueueSessionContactVerification, readSessionContactVerification, acknowledgeSessionContactVerification } from "../lib/matrixToDevice.js";
import { ensureSessionDirectMatrixRoom, sessionMatrixRoomSnapshot, issueSessionMatrixEventTransaction, sendSessionMatrixRoomEvent, sessionMatrixRoomHistory, readSessionMatrixRoomAttachment, setSessionMatrixRoomEventPinned, deleteSessionMatrixRoomEvent } from "../lib/matrixRooms.js";

export function createMatrixMessagingRouter(serverName, { notify = () => {} } = {}) {
  if (typeof serverName !== "string" || !/^[a-z0-9](?:[a-z0-9.-]{0,198}[a-z0-9])?$/.test(serverName) || serverName.includes("..")) throw new Error("Invalid MATRIX_SERVER_NAME");
  const router = express.Router();
  router.use(protectRoute, createAuthRateLimit({ limit: 240, windowMs: 60000 }));
  router.use((req, res, next) => { res.set("Cache-Control", "no-store"); if (!req.sessionId) return res.status(401).json({ message: "Sign in again to use this experimental feature." }); next(); });
  const safe = fn => async (req, res) => {
    try { res.json(await fn(req, String(req.user._id), req.sessionId)); }
    catch { res.status(400).json({ message: "Encrypted messaging operation failed." }); }
  };
  router.post("/rooms/direct", safe((req, a, s) => ensureSessionDirectMatrixRoom(a, s, serverName, req.body?.deviceId, req.body?.peerAccountId)));
  router.post("/keys/query", safe((req, a, s) => querySessionRoomDeviceKeys(a, s, serverName, req.body?.deviceId, req.body?.roomId, req.body?.revision, req.body?.users)));
  router.post("/keys/claim", safe((req, a, s) => claimSessionRoomOneTimeKey(a, s, serverName, req.body?.deviceId, req.body?.roomId, req.body?.revision, req.body?.targetUser, req.body?.targetDevice, req.body?.algorithm)));
  router.post("/shares/send", safe(async (req, a, s) => { const b = req.body || {}; await enqueueSessionRoomKeyShares(a, s, serverName, b.roomId, b.revision, b.deviceId, b.transactionId, b.eventType, b.messages); return { accepted: true }; }));
  router.get("/shares/inbox", safe(async (req, a, s) => ({ events: await readSessionRoomToDevice(a, s, serverName, req.query.deviceId, req.query.roomId) })));
  router.post("/shares/ack", safe(async (req, a, s) => { await acknowledgeSessionRoomToDevice(a, s, serverName, req.body?.deviceId, req.body?.roomId, req.body?.ids); return { acknowledged: true }; }));
  router.post("/verification/send", safe(async (req, a, s) => { const b = req.body || {}; await enqueueSessionContactVerification(a, s, serverName, b.roomId, b.revision, b.deviceId, b.transactionId, b.eventType, b.messages); return { accepted: true }; }));
  router.get("/verification/inbox", safe(async (req, a, s) => ({ events: await readSessionContactVerification(a, s, serverName, req.query.deviceId, req.query.roomId) })));
  router.post("/verification/ack", safe(async (req, a, s) => { await acknowledgeSessionContactVerification(a, s, serverName, req.body?.deviceId, req.body?.roomId, req.body?.ids); return { acknowledged: true }; }));
  router.post("/events/transaction", safe((req, a, s) => issueSessionMatrixEventTransaction(a, s, serverName, req.body?.roomId, req.body?.deviceId, req.body?.revision)));
  router.post("/events/send", safe(async (req, a, s) => {
    const saved = await sendSessionMatrixRoomEvent(a, s, serverName, req.body?.roomId, req.body?.deviceId, req.body?.revision, req.body?.content, req.body?.token);
    // A notification contains no message or room data. Recipients must fetch
    // history through the normal session, device and membership checks.
    if (Array.isArray(saved.visibleTo)) { try { notify(saved.visibleTo); } catch { /* persistence succeeded; clients can refresh */ } }
    return saved;
  }));
  // Binary frame: four-byte big-endian JSON length, bounded JSON envelope,
  // then encrypted bytes. Keys and filenames belong only in the encrypted event.
  router.post("/events/send-attachment", express.raw({ type: "application/octet-stream", limit: 10 * 1024 * 1024 + 65540, inflate: false }), safe(async (req, a, s) => {
    if (!Buffer.isBuffer(req.body) || req.body.length < 6) throw new Error("Invalid attachment frame");
    const length = req.body.readUInt32BE(0);
    if (length === 0 || length > 65536 || 4 + length >= req.body.length) throw new Error("Invalid attachment frame");
    const header = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(req.body.subarray(4, 4 + length)));
    const keys = ["roomId", "deviceId", "revision", "content", "token", "attachmentId"];
    if (!header || typeof header !== "object" || Array.isArray(header) || Object.keys(header).length !== keys.length || keys.some(key => !Object.hasOwn(header, key))) throw new Error("Invalid attachment frame");
    const saved = await sendSessionMatrixRoomEvent(a, s, serverName, header.roomId, header.deviceId, header.revision, header.content, header.token, req.body.subarray(4 + length), header.attachmentId);
    if (Array.isArray(saved.visibleTo)) { try { notify(saved.visibleTo); } catch { /* clients can refresh */ } }
    return saved;
  }));
  router.get("/events/attachment", async (req, res) => {
    try {
      const identifier = req.query.eventId || req.query.attachmentId;
      const bytes = await readSessionMatrixRoomAttachment(String(req.user._id), req.sessionId, serverName, req.query.roomId, req.query.deviceId, identifier);
      res.set({ "Content-Type": "application/octet-stream", "Content-Disposition": "attachment", "X-Content-Type-Options": "nosniff", "Cache-Control": "no-store" }).send(bytes);
    } catch { res.status(400).json({ message: "Encrypted attachment unavailable." }); }
  });
  router.get("/events/history", safe((req, a, s) => sessionMatrixRoomHistory(a, s, serverName, req.query.roomId, req.query.deviceId, { before: req.query.before, limit: req.query.limit, pinned: req.query.pinned })));
  router.post("/events/pin", safe(async (req, a, s) => {
    const b = req.body || {}, room = await sessionMatrixRoomSnapshot(a, s, serverName, b.roomId, b.deviceId);
    const result = await setSessionMatrixRoomEventPinned(a, s, serverName, b.roomId, b.deviceId, b.eventId, b.pinned);
    try { notify(room.members); } catch { /* clients can refresh */ }
    return result;
  }));
  router.post("/events/delete", safe(async (req, a, s) => {
    const b = req.body || {}, room = await sessionMatrixRoomSnapshot(a, s, serverName, b.roomId, b.deviceId);
    const result = await deleteSessionMatrixRoomEvent(a, s, serverName, b.roomId, b.deviceId, b.eventId);
    try { notify(room.members); } catch { /* clients can refresh */ }
    return result;
  }));
  return router;
}
