// Storage primitives. The experimental HTTP gateway exposes only the
// session-bound variants; room provisioning remains internal.
import { createHash, randomBytes } from "node:crypto";
import mongoose from "mongoose";
import MatrixRoom from "../models/matrixRoom.model.js";
import MatrixRoomEvent from "../models/matrixRoomEvent.model.js";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixRoomSession from "../models/matrixRoomSession.model.js";
import MatrixEventTransaction from "../models/matrixEventTransaction.model.js";
import MatrixStorageBudget from "../models/matrixStorageBudget.model.js";
import User from "../models/user.model.js";
import { WEEK } from "../models/message.model.js";
import { historyQuery, historyPage, pinUpdate } from "./history.js";
import { activeAuthorizedMatrixDevice, matrixSessionIdentity } from "./matrixSessionBinding.js";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const ROOM = /^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const ID = /^[a-f0-9]{24}$/;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;
const hash = value => createHash("sha256").update(value).digest("hex");
const validUser = value => typeof value === "string" && USER.test(value);
const validRoom = value => typeof value === "string" && ROOM.test(value);
function attachmentGlobalLimit() {
  const configured = process.env.MATRIX_ATTACHMENT_GLOBAL_LIMIT_BYTES;
  if (configured === undefined || configured === "") return 1024 * 1024 * 1024;
  if (!/^[1-9][0-9]{0,10}$/.test(configured)) throw new Error("Invalid attachment storage limit");
  const limit = Number(configured);
  if (!Number.isSafeInteger(limit) || limit > 10 * 1024 * 1024 * 1024) throw new Error("Invalid attachment storage limit");
  return limit;
}
const validDevice = value => typeof value === "string" && DEVICE.test(value);
const object = value => value !== null && typeof value === "object" && !Array.isArray(value);
const live = () => ({ $or: [{ pinned: true }, { expiresAt: { $gt: new Date() } }] });
function key64(value) {
  if (typeof value !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(value)) return false;
  const bytes = Buffer.from(value, "base64");
  return bytes.length === 32 && bytes.toString("base64").replace(/=+$/, "") === value;
}
function encryptedContent(value, deviceId) {
  const fields = ["algorithm", "ciphertext", "device_id", "sender_key", "session_id"];
  if (!object(value) || Object.keys(value).length !== fields.length || !fields.every(field => Object.hasOwn(value, field)) || value.algorithm !== "m.megolm.v1.aes-sha2" || value.device_id !== deviceId || !key64(value.sender_key) || !key64(value.session_id) || typeof value.ciphertext !== "string" || value.ciphertext.length < 24 || value.ciphertext.length > 48000 || !/^[A-Za-z0-9+/]+$/.test(value.ciphertext)) throw new Error("Invalid encrypted room event");
  return Object.fromEntries(fields.map(field => [field, value[field]]));
}
function membersList(owner, members, kind) {
  if (!validUser(owner) || !Array.isArray(members) || members.length < 2 || members.length > 32 || members.some(member => !validUser(member)) || new Set(members).size !== members.length || !members.includes(owner) || !["direct", "community"].includes(kind) || (kind === "direct" && members.length !== 2)) throw new Error("Invalid room members");
  return [...members].sort();
}
export async function createMatrixRoom(roomId, owner, members, kind) {
  if (!validRoom(roomId)) throw new Error("Invalid room");
  return MatrixRoom.create({ roomId, owner, members: membersList(owner, members, kind), kind });
}

// Both participants derive the same room from their account IDs. Only a
// tracked, authorized device may provision it; no client supplies membership
// or the room ID. Existing rooms are never silently rewritten.
export async function ensureSessionDirectMatrixRoom(accountId, sessionId, serverName, deviceId, peerAccountId) {
  const owner = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof peerAccountId !== "string" || !ID.test(peerAccountId) || peerAccountId === accountId) throw new Error("Invalid peer");
  const peer = `@${peerAccountId}:${serverName}`;
  const members = [owner, peer].sort();
  const roomId = `!${hash(JSON.stringify(["sonar-direct-v1", ...members]))}:${serverName}`;
  let snapshot;
  const create = () => mongoose.connection.transaction(async session => {
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, owner, deviceId);
    if (!await User.exists({ _id: peerAccountId }).session(session)) throw new Error("Peer unavailable");
    const room = await MatrixRoom.findOneAndUpdate({ roomId }, { $setOnInsert: { roomId, owner: members[0], members, kind: "direct", revision: 0 }, $inc: { activityVersion: 1 } }, { session, upsert: true, new: true, setDefaultsOnInsert: false }).lean();
    if (!room || room.kind !== "direct" || room.members.length !== 2 || room.members[0] !== members[0] || room.members[1] !== members[1]) throw new Error("Room identity changed");
    snapshot = { roomId, revision: room.revision, members: room.members, kind: room.kind };
  });
  try { await create(); } catch (error) { if (error.code !== 11000) throw error; await create(); }
  return snapshot;
}
export async function setMatrixRoomMembers(roomId, owner, expectedRevision, members) {
  if (!validRoom(roomId) || !Number.isSafeInteger(expectedRevision) || expectedRevision < 0) throw new Error("Invalid room revision");
  const sorted = membersList(owner, members, "community");
  const updated = await MatrixRoom.findOneAndUpdate({ roomId, owner, kind: "community", revision: expectedRevision }, { $set: { members: sorted }, $inc: { revision: 1 } }, { new: true });
  if (!updated) throw new Error("Room membership changed");
  return updated;
}

// A client must use this snapshot's revision for key lookup, sharing and send.
// The server name comes from operator configuration, not the request body.
export async function sessionMatrixRoomSnapshot(accountId, sessionId, serverName, roomId, deviceId) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (!validRoom(roomId)) throw new Error("Invalid room");
  let snapshot;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: userId }, { $inc: { activityVersion: 1 } }, { session, new: true }).lean();
    if (!room) throw new Error("Room unavailable");
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, userId, deviceId);
    snapshot = { roomId: room.roomId, revision: room.revision, members: room.members, kind: room.kind };
  });
  return snapshot;
}
// Issue once, then reuse the opaque token for every retry of the same event.
// A missing/expired receipt is never treated as permission to create an event.
export async function issueMatrixEventTransaction(roomId, sender, deviceId, revision) {
  return issueEventTransaction(roomId, sender, deviceId, revision, null);
}
export async function issueSessionMatrixEventTransaction(accountId, sessionId, serverName, roomId, deviceId, revision) {
  return issueEventTransaction(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, revision, { accountId, sessionId });
}
async function issueEventTransaction(roomId, sender, deviceId, revision, auth) {
  if (!validRoom(roomId) || !validUser(sender) || !validDevice(deviceId) || !Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid sender or room");
  const token = randomBytes(32).toString("base64url");
  const expiresAt = new Date(Date.now() + 24 * 60 * 60 * 1000);
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: sender, revision }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room membership changed");
    const device = auth ? await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, sender, deviceId) : await MatrixDevice.findOneAndUpdate({ userId: sender, deviceId, serverAuthorization: "authorized", revokedAt: null }, { $inc: { revision: 1 } }, { session });
    if (!device) throw new Error("Sender device unavailable");
    if (await MatrixEventTransaction.countDocuments({ sender, senderDevice: deviceId, expiresAt: { $gt: new Date() } }).session(session) >= 10000) throw new Error("Transaction capacity reached");
    await MatrixEventTransaction.create([{ tokenHash: hash(token), roomId, sender, senderDevice: deviceId, revision, expiresAt }], { session });
  });
  return { token, expiresAt };
}
export async function sendMatrixRoomEvent(roomId, sender, deviceId, expectedRevision, input, token, attachment, attachmentId) {
  return sendRoomEvent(roomId, sender, deviceId, expectedRevision, input, token, null, attachment, attachmentId);
}
export async function sendSessionMatrixRoomEvent(accountId, sessionId, serverName, roomId, deviceId, revision, input, token, attachment, attachmentId) {
  return sendRoomEvent(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, revision, input, token, { accountId, sessionId }, attachment, attachmentId);
}
async function sendRoomEvent(roomId, sender, deviceId, expectedRevision, input, token, auth, attachment, attachmentId) {
  if (!validRoom(roomId) || !validUser(sender) || !validDevice(deviceId) || !Number.isSafeInteger(expectedRevision) || expectedRevision < 0 || typeof token !== "string" || !TOKEN.test(token)) throw new Error("Invalid sender, room, or transaction");
  const content = encryptedContent(input, deviceId);
  if ((attachment === undefined) !== (attachmentId === undefined) || (attachment !== undefined && (!Buffer.isBuffer(attachment) || attachment.length === 0 || attachment.length > 10 * 1024 * 1024 || typeof attachmentId !== "string" || !/^[a-f0-9]{32}$/.test(attachmentId)))) throw new Error("Invalid encrypted attachment");
  const contentHash = attachment === undefined ? hash(JSON.stringify(content)) : hash(`${JSON.stringify(content)}\0${attachmentId}\0${hash(attachment)}`);
  let saved;
  await mongoose.connection.transaction(async session => {
    // Both writes serialize this send against membership changes and revocation.
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: sender }, { $inc: { activityVersion: 1 } }, { session, new: true });
    if (!room) throw new Error("Room membership changed");
    const device = auth ? await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, sender, deviceId) : await MatrixDevice.findOneAndUpdate({ userId: sender, deviceId, serverAuthorization: "authorized", revokedAt: null }, { $inc: { revision: 1 } }, { session, new: true });
    if (!device || device.deviceKeys.keys[`curve25519:${deviceId}`] !== content.sender_key) throw new Error("Sender device unavailable");
    const receipt = await MatrixEventTransaction.findOne({ tokenHash: hash(token), roomId, sender, senderDevice: deviceId, revision: expectedRevision, expiresAt: { $gt: new Date() } }).session(session).lean();
    if (!receipt) throw new Error("Transaction unavailable or expired");
    if (receipt.eventId) {
      if (receipt.contentHash !== contentHash) throw new Error("Transaction cannot be replaced");
      saved = await MatrixRoomEvent.findOne({ _id: receipt.eventId, roomId, sender, ...live() }).session(session).lean();
      saved = saved ? { ...saved, duplicate: true } : { _id: receipt.eventId, duplicate: true, unavailable: true };
      return;
    }
    if (room.revision !== expectedRevision) throw new Error("Room membership changed");
    const selector = { roomId, sender, senderDevice: deviceId, sessionId: content.session_id };
    const existing = await MatrixRoomSession.findOne(selector).session(session).lean();
    if (existing && existing.revision !== expectedRevision) throw new Error("Room key must rotate after membership changes");
    if (!existing) {
      if (await MatrixRoomSession.countDocuments({ roomId, sender, senderDevice: deviceId }).session(session) >= 100000) throw new Error("Room key history capacity reached");
      await MatrixRoomSession.create([{ ...selector, revision: expectedRevision }], { session });
    }
    if (attachment !== undefined) {
      const limit = attachmentGlobalLimit();
      await MatrixStorageBudget.findOneAndUpdate({ _id: "attachments" }, { $inc: { revision: 1 } }, { upsert: true, session });
      const size = { $binarySize: { $ifNull: ["$attachmentCiphertext", ""] } };
      const [usage] = await MatrixRoomEvent.aggregate([{ $group: { _id: null, total: { $sum: size }, room: { $sum: { $cond: [{ $eq: ["$roomId", roomId] }, size, 0] } } } }]).session(session);
      if ((usage?.room || 0) + attachment.length > 100 * 1024 * 1024) throw new Error("Room attachment capacity reached");
      if ((usage?.total || 0) + attachment.length > limit) throw new Error("Global attachment capacity reached");
    }
    [saved] = await MatrixRoomEvent.create([{ roomId, sender, senderDevice: deviceId, visibleTo: [...room.members], content, ...(attachment === undefined ? {} : { attachmentCiphertext: attachment, attachmentId }) }], { session });
    await MatrixEventTransaction.updateOne({ _id: receipt._id, eventId: null }, { $set: { eventId: saved._id, contentHash } }, { session });
    saved = saved.toObject();
    delete saved.attachmentCiphertext;
  });
  return saved;
}
export async function readMatrixRoomAttachment(roomId, reader, eventId) {
  return readRoomAttachment(roomId, reader, eventId, null);
}
export async function readSessionMatrixRoomAttachment(accountId, sessionId, serverName, roomId, deviceId, eventId) {
  return readRoomAttachment(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), eventId, { accountId, sessionId, deviceId });
}
async function readRoomAttachment(roomId, reader, eventId, auth) {
  if (!validRoom(roomId) || !validUser(reader) || typeof eventId !== "string" || (!ID.test(eventId) && !/^[a-f0-9]{32}$/.test(eventId))) throw new Error("Invalid attachment request");
  let bytes;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: reader }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room unavailable");
    if (auth) await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, reader, auth.deviceId);
    const selector = ID.test(eventId) ? { _id: eventId } : { attachmentId: eventId };
    const event = await MatrixRoomEvent.findOne({ ...selector, roomId, visibleTo: reader, ...live() }).select("+attachmentCiphertext").session(session).lean();
    if (!event?.attachmentCiphertext) throw new Error("Attachment unavailable");
    const data = event.attachmentCiphertext;
    bytes = Buffer.isBuffer(data) ? Buffer.from(data) : Buffer.from(data.value(true));
  });
  return bytes;
}
export async function matrixRoomHistory(roomId, reader, query = {}) {
  return roomHistory(roomId, reader, query, null);
}
export async function sessionMatrixRoomHistory(accountId, sessionId, serverName, roomId, deviceId, query = {}) {
  return roomHistory(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), query, { accountId, sessionId, deviceId });
}
async function roomHistory(roomId, reader, query, auth) {
  if (!validRoom(roomId) || !validUser(reader)) throw new Error("Invalid room or reader");
  const options = historyQuery({ ...query, page: "true" });
  let rows;
  await mongoose.connection.transaction(async session => {
    // Serialize the permission check with membership removal. A snapshot-only
    // read followed by a separate query can return a page after access changed.
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: reader }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room unavailable");
    if (auth) await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, reader, auth.deviceId);
    rows = await MatrixRoomEvent.find({ roomId, visibleTo: reader, $and: [live(), ...options.filters] }).sort({ createdAt: -1, _id: -1 }).limit(options.limit + 1).session(session).lean();
  });
  return historyPage(rows, options.limit);
}
export async function setMatrixRoomEventPinned(roomId, actor, eventId, pinned) {
  return setRoomEventPinned(roomId, actor, eventId, pinned, null);
}
export async function setSessionMatrixRoomEventPinned(accountId, sessionId, serverName, roomId, deviceId, eventId, pinned) {
  return setRoomEventPinned(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), eventId, pinned, { accountId, sessionId, deviceId });
}
async function setRoomEventPinned(roomId, actor, eventId, pinned, auth) {
  if (!validRoom(roomId) || !validUser(actor) || typeof eventId !== "string" || !ID.test(eventId) || typeof pinned !== "boolean") throw new Error("Invalid pin request");
  let updated;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: actor }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room unavailable");
    if (auth) await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, actor, auth.deviceId);
    updated = await MatrixRoomEvent.findOneAndUpdate({ _id: eventId, roomId, visibleTo: actor, ...live() }, pinUpdate(pinned, WEEK), { session, new: true }).lean();
    if (!updated) throw new Error("Event unavailable");
  });
  return updated;
}
export async function deleteMatrixRoomEvent(roomId, actor, eventId) {
  return deleteRoomEvent(roomId, actor, eventId, null);
}
export async function deleteSessionMatrixRoomEvent(accountId, sessionId, serverName, roomId, deviceId, eventId) {
  return deleteRoomEvent(roomId, matrixSessionIdentity(accountId, sessionId, serverName, deviceId), eventId, { accountId, sessionId, deviceId });
}
async function deleteRoomEvent(roomId, actor, eventId, auth) {
  if (!validRoom(roomId) || !validUser(actor) || typeof eventId !== "string" || !ID.test(eventId)) throw new Error("Invalid delete request");
  let deleted;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, members: actor }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room unavailable");
    if (auth) await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, actor, auth.deviceId);
    deleted = await MatrixRoomEvent.findOneAndDelete({ _id: eventId, roomId, sender: actor, ...live() }, { session }).lean();
    if (!deleted) throw new Error("Event unavailable");
  });
  return { id: String(deleted._id) };
}
export async function deleteMatrixCommunityRoom(roomId, owner) {
  if (!validRoom(roomId) || !validUser(owner)) throw new Error("Invalid room");
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndDelete({ roomId, owner, kind: "community" }, { session });
    if (!room) throw new Error("Room unavailable");
    await MatrixRoomEvent.deleteMany({ roomId }, { session });
    await MatrixRoomSession.deleteMany({ roomId }, { session });
    await MatrixEventTransaction.deleteMany({ roomId }, { session });
  });
}
