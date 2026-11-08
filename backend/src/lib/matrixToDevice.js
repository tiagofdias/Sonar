// Internal transport primitives only. They are not mounted on HTTP: callers
// must first bind the authenticated session to an approved Matrix device and
// authorize every recipient. This is not a production sync endpoint.
import crypto from "node:crypto";
import mongoose from "mongoose";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixToDevice from "../models/matrixToDevice.model.js";
import MatrixToDeviceTransaction from "../models/matrixToDeviceTransaction.model.js";
import MatrixRoom from "../models/matrixRoom.model.js";
import { activeAuthorizedMatrixDevice, activeBoundMatrixDevice, matrixSessionIdentity } from "./matrixSessionBinding.js";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const TRANSACTION = /^[A-Za-z0-9._~-]{1,128}$/;
const ID = /^[a-f0-9]{24}$/;
const WEEK = 7 * 24 * 60 * 60 * 1000;
const ROOM = /^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/;
const isObject = value => value !== null && typeof value === "object" && !Array.isArray(value);
const identity = (user, device) => typeof user === "string" && USER.test(user) && typeof device === "string" && DEVICE.test(device);
const exact = (value, keys) => isObject(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const canonical = value => Array.isArray(value) ? `[${value.map(canonical).join(",")}]` : isObject(value) ? `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}` : JSON.stringify(value);
const key64 = value => typeof value === "string" && /^[A-Za-z0-9+/]{43}$/.test(value) && Buffer.from(value, "base64").length === 32 && Buffer.from(value, "base64").toString("base64").replace(/=+$/, "") === value;

function parseMessages(messages) {
  if (!isObject(messages)) throw new Error("Invalid encrypted events");
  const recipients = [];
  for (const [userId, devices] of Object.entries(messages)) {
    if (!isObject(devices)) throw new Error("Invalid encrypted events");
    for (const [deviceId, content] of Object.entries(devices)) {
      const hasMessageId = isObject(content) && Object.hasOwn(content, "org.matrix.msgid");
      const fields = hasMessageId ? ["algorithm", "sender_key", "ciphertext", "org.matrix.msgid"] : ["algorithm", "sender_key", "ciphertext"];
      if (!identity(userId, deviceId) || !exact(content, fields) || (hasMessageId && (typeof content["org.matrix.msgid"] !== "string" || !TRANSACTION.test(content["org.matrix.msgid"]))) || content.algorithm !== "m.olm.v1.curve25519-aes-sha2" || !key64(content.sender_key) || !isObject(content.ciphertext)) throw new Error("Invalid encrypted event");
      const entries = Object.entries(content.ciphertext);
      if (entries.length !== 1) throw new Error("Invalid encrypted recipient");
      const [key, cipher] = entries[0];
      if (!key64(key) || !exact(cipher, ["type", "body"]) || ![0, 1].includes(cipher.type) || typeof cipher.body !== "string" || cipher.body.length < 1 || cipher.body.length > 65536 || !/^[A-Za-z0-9+/]+$/.test(cipher.body)) throw new Error("Invalid ciphertext");
      recipients.push({ userId, deviceId, recipientKey: key, content: { algorithm: content.algorithm, sender_key: content.sender_key, ciphertext: { [key]: { type: cipher.type, body: cipher.body } }, ...(hasMessageId ? { "org.matrix.msgid": content["org.matrix.msgid"] } : {}) } });
      if (recipients.length > 100) throw new Error("Too many recipients");
    }
  }
  if (!recipients.length) throw new Error("No recipients");
  return recipients;
}

export async function enqueueToDevice(sender, senderDevice, transactionId, eventType, messages) {
  return enqueue(sender, senderDevice, transactionId, eventType, messages, null);
}

// This restricted route is for room-key sharing. Ciphertext is opaque, so the
// server cannot prove the encrypted event is a room key or stop a malicious
// authorized participant leaking keys elsewhere. Pairing uses a separate path.
export async function enqueueRoomKeyShares(roomId, revision, sender, senderDevice, transactionId, eventType, messages) {
  if (typeof roomId !== "string" || !ROOM.test(roomId) || !Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid room scope");
  return enqueue(sender, senderDevice, transactionId, eventType, messages, { roomId, revision });
}

// The future HTTP gateway must take accountId/sessionId from authentication and
// serverName from operator configuration, never from request-provided identity.
// Verify the session and perform the queue write in the same transaction.
export async function enqueueSessionRoomKeyShares(accountId, sessionId, serverName, roomId, revision, deviceId, transactionId, eventType, messages) {
  const sender = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof roomId !== "string" || !ROOM.test(roomId) || !Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid room session");
  return enqueue(sender, deviceId, transactionId, eventType, messages, { roomId, revision }, { accountId, sessionId });
}
// The payload is encrypted: the service cannot inspect whether it is really a
// SAS event. Restrict it to one other device on the same account and require an
// already authorized endpoint. Client SDK verification is still mandatory.
export async function enqueueSessionVerification(accountId, sessionId, serverName, deviceId, transactionId, eventType, messages) {
  const sender = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  return enqueue(sender, deviceId, transactionId, eventType, messages, { purpose: "verification" }, { accountId, sessionId });
}
// Opaque contact-verification events remain scoped to one other account in a
// current room; both devices must already be authorized for their accounts.
export async function enqueueSessionContactVerification(accountId, sessionId, serverName, roomId, revision, deviceId, transactionId, eventType, messages) {
  const sender = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof roomId !== "string" || !ROOM.test(roomId) || !Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid room session");
  return enqueue(sender, deviceId, transactionId, eventType, messages, { roomId, revision, purpose: "contact-verification" }, { accountId, sessionId });
}

async function enqueue(sender, senderDevice, transactionId, eventType, messages, scope, auth = null) {
  if (!identity(sender, senderDevice) || typeof transactionId !== "string" || !TRANSACTION.test(transactionId) || eventType !== "m.room.encrypted") throw new Error("Invalid transaction");
  const recipients = parseMessages(messages);
  const verification = scope?.purpose === "verification";
  const contactVerification = scope?.purpose === "contact-verification";
  if (verification && (recipients.length !== 1 || recipients[0].userId !== sender || recipients[0].deviceId === senderDevice)) throw new Error("Invalid verification recipient");
  if (contactVerification && (recipients.length !== 1 || recipients[0].userId === sender)) throw new Error("Invalid contact verification recipient");
  const digest = crypto.createHash("sha256").update(canonical({ eventType, messages, scope })).digest("hex");
  await mongoose.connection.transaction(async session => {
    const now = new Date();
    if (scope && !verification) {
      const recipientsUsers = [...new Set([sender, ...recipients.map(recipient => recipient.userId)])];
      const room = await MatrixRoom.findOneAndUpdate({ roomId: scope.roomId, revision: scope.revision, members: { $all: recipientsUsers } }, { $inc: { activityVersion: 1 } }, { session });
      if (!room) throw new Error("Room membership changed");
    }
    const device = auth ? (verification ? await activeBoundMatrixDevice(session, auth.accountId, auth.sessionId, sender, senderDevice, ["authorized", "pending"]) : await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, sender, senderDevice)) : await MatrixDevice.findOneAndUpdate({ userId: sender, deviceId: senderDevice, revokedAt: null, ...(scope ? { serverAuthorization: "authorized" } : {}) }, { $inc: { revision: 1 } }, { session, new: true });
    if (!device) throw new Error("Unknown sender device");
    const existing = await MatrixToDeviceTransaction.findOne({ sender, deviceId: senderDevice, transactionId }).session(session).lean();
    if (existing) {
      if (existing.digest !== digest) throw new Error("Transaction cannot be replaced");
      return;
    }
    if (await MatrixToDeviceTransaction.countDocuments({ sender, deviceId: senderDevice, expiresAt: { $gt: now } }).session(session) >= 10000) throw new Error("Transaction capacity reached");
    const senderKey = device.deviceKeys.keys[`curve25519:${senderDevice}`];
    const records = [];
    for (const recipient of recipients) {
      // Lock device records to serialize enqueue with device revocation.
      const target = await MatrixDevice.findOneAndUpdate({ userId: recipient.userId, deviceId: recipient.deviceId, revokedAt: null, ...(verification ? { serverAuthorization: { $in: ["authorized", "pending"] } } : scope ? { serverAuthorization: "authorized" } : {}) }, { $inc: { revision: 1 } }, { session, new: true });
      if (!target || target.deviceKeys.keys[`curve25519:${recipient.deviceId}`] !== recipient.recipientKey || recipient.content.sender_key !== senderKey) throw new Error("Device key mismatch");
      if (verification && device.serverAuthorization !== "authorized" && target.serverAuthorization !== "authorized") throw new Error("Authorized device required");
      const purpose = verification ? "verification" : contactVerification ? "contact-verification" : "general";
      if (await MatrixToDevice.countDocuments({ recipient: recipient.userId, deviceId: recipient.deviceId, purpose, expiresAt: { $gt: now } }).session(session) >= (verification || contactVerification ? 50 : 1000)) throw new Error("Recipient queue full");
      records.push({ sender, recipient: recipient.userId, deviceId: recipient.deviceId, purpose, roomId: scope && !verification ? scope.roomId : null, content: recipient.content, expiresAt: new Date(now.getTime() + (verification || contactVerification ? 600000 : WEEK)) });
    }
    await MatrixToDevice.insertMany(records, { session });
    await MatrixToDeviceTransaction.create([{ sender, deviceId: senderDevice, transactionId, digest, expiresAt: new Date(now.getTime() + WEEK) }], { session });
  });
}

export async function readToDevice(userId, deviceId) {
  return readDeviceEvents(userId, deviceId, null);
}
export async function readSessionToDevice(accountId, sessionId, serverName, deviceId) {
  return readDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, { accountId, sessionId, purpose: "general" });
}
export async function readSessionRoomToDevice(accountId, sessionId, serverName, deviceId, roomId) {
  if (typeof roomId !== "string" || !ROOM.test(roomId)) throw new Error("Invalid room");
  return readDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, { accountId, sessionId, purpose: "general", roomId });
}
export async function readSessionContactVerification(accountId, sessionId, serverName, deviceId, roomId) {
  if (typeof roomId !== "string" || !ROOM.test(roomId)) throw new Error("Invalid room");
  return readDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, { accountId, sessionId, purpose: "contact-verification", roomId });
}
export async function readSessionVerification(accountId, sessionId, serverName, deviceId) {
  return readDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, { accountId, sessionId, purpose: "verification" });
}
async function readDeviceEvents(userId, deviceId, auth) {
  if (!identity(userId, deviceId)) throw new Error("Invalid device");
  let rows;
  await mongoose.connection.transaction(async session => {
    if (auth?.roomId && !await MatrixRoom.findOneAndUpdate({ roomId: auth.roomId, members: userId }, { $inc: { activityVersion: 1 } }, { session })) throw new Error("Room unavailable");
    const device = auth ? (auth.purpose === "verification" ? await activeBoundMatrixDevice(session, auth.accountId, auth.sessionId, userId, deviceId, ["authorized", "pending"]) : await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, userId, deviceId)) : await MatrixDevice.findOneAndUpdate({ userId, deviceId, revokedAt: null }, { $inc: { revision: 1 } }, { session });
    if (!device) throw new Error("Unknown device");
    rows = await MatrixToDevice.find({ recipient: userId, deviceId, ...(auth ? { purpose: auth.purpose } : {}), ...(auth?.roomId ? { roomId: auth.roomId } : {}), expiresAt: { $gt: new Date() } }).sort({ _id: 1 }).limit(100).session(session).lean();
  });
  return rows.map(row => ({ id: String(row._id), event: { type: "m.room.encrypted", sender: row.sender, content: row.content } }));
}

export async function acknowledgeToDevice(userId, deviceId, ids) {
  return acknowledgeDeviceEvents(userId, deviceId, ids, null);
}
export async function acknowledgeSessionToDevice(accountId, sessionId, serverName, deviceId, ids) {
  return acknowledgeDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, ids, { accountId, sessionId, purpose: "general" });
}
export async function acknowledgeSessionRoomToDevice(accountId, sessionId, serverName, deviceId, roomId, ids) {
  if (typeof roomId !== "string" || !ROOM.test(roomId)) throw new Error("Invalid room");
  return acknowledgeDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, ids, { accountId, sessionId, purpose: "general", roomId });
}
export async function acknowledgeSessionContactVerification(accountId, sessionId, serverName, deviceId, roomId, ids) {
  if (typeof roomId !== "string" || !ROOM.test(roomId)) throw new Error("Invalid room");
  return acknowledgeDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, ids, { accountId, sessionId, purpose: "contact-verification", roomId });
}
export async function acknowledgeSessionVerification(accountId, sessionId, serverName, deviceId, ids) {
  return acknowledgeDeviceEvents(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, ids, { accountId, sessionId, purpose: "verification" });
}
async function acknowledgeDeviceEvents(userId, deviceId, ids, auth) {
  if (!identity(userId, deviceId) || !Array.isArray(ids) || ids.length > 100 || ids.some(id => typeof id !== "string" || !ID.test(id))) throw new Error("Invalid acknowledgement");
  await mongoose.connection.transaction(async session => {
    if (auth?.roomId && !await MatrixRoom.findOneAndUpdate({ roomId: auth.roomId, members: userId }, { $inc: { activityVersion: 1 } }, { session })) throw new Error("Room unavailable");
    const device = auth ? (auth.purpose === "verification" ? await activeBoundMatrixDevice(session, auth.accountId, auth.sessionId, userId, deviceId, ["authorized", "pending"]) : await activeAuthorizedMatrixDevice(session, auth.accountId, auth.sessionId, userId, deviceId)) : await MatrixDevice.findOneAndUpdate({ userId, deviceId, revokedAt: null }, { $inc: { revision: 1 } }, { session });
    if (!device) throw new Error("Unknown device");
    await MatrixToDevice.deleteMany({ recipient: userId, deviceId, ...(auth ? { purpose: auth.purpose } : {}), ...(auth?.roomId ? { roomId: auth.roomId } : {}), _id: { $in: ids } }, { session });
  });
}
