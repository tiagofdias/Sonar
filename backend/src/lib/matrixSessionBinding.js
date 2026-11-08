// Internal authentication building block; not mounted on HTTP. A signed
// challenge proves possession of a Matrix device key, not that the device has
// been verified or approved. Device verification remains a separate gate. A
// gateway must derive the Matrix server name from trusted configuration rather
// than accepting the user ID supplied by the client.
import { createHash, randomBytes } from "node:crypto";
import mongoose from "mongoose";
import LoginSession from "../models/loginSession.model.js";
import User from "../models/user.model.js";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixBindingChallenge from "../models/matrixBindingChallenge.model.js";
import { verifyMatrixEd25519 } from "./matrixSignatures.js";

const ACCOUNT = /^[a-f0-9]{24}$/;
const SESSION = /^[a-f0-9]{64}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const USER = /^@([a-f0-9]{24}):[a-z0-9.-]{1,200}$/;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;
const hash = value => createHash("sha256").update(value).digest("hex");
const bindingMessage = (userId, deviceId, sessionId, token) => `sonar-matrix-device-binding-v1\n${userId}\n${deviceId}\n${sessionId}\n${token}`;
export function matrixSessionIdentity(accountId, sessionId, serverName, deviceId) {
  if (typeof serverName !== "string" || !/^[a-z0-9](?:[a-z0-9.-]{0,198}[a-z0-9])?$/.test(serverName) || serverName.includes("..")) throw new Error("Invalid server identity");
  const userId = `@${accountId}:${serverName}`;
  validate(accountId, sessionId, userId, deviceId);
  return userId;
}
function validate(accountId, sessionId, userId, deviceId) {
  if (typeof accountId !== "string" || !ACCOUNT.test(accountId) || typeof sessionId !== "string" || !SESSION.test(sessionId) || typeof userId !== "string" || USER.exec(userId)?.[1] !== accountId || typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid binding identity");
}
export async function activeMatrixLogin(session, accountId, sessionId) {
  const login = await LoginSession.findOneAndUpdate({ _id: sessionId, userId: accountId, expiresAt: { $gt: new Date() } }, { $inc: { matrixChallengeRevision: 1 } }, { session, new: true });
  const version = login?.sessionVersion === 0 ? { $or: [{ sessionVersion: 0 }, { sessionVersion: { $exists: false } }] } : { sessionVersion: login?.sessionVersion };
  // The write forces a conflict with password changes and global sign-out.
  // A snapshot-only read could otherwise let a stale transaction commit after
  // sessionVersion was incremented on a different connection.
  if (!login || !await User.findOneAndUpdate({ _id: accountId, ...version }, { $inc: { matrixRevision: 1 } }, { session, timestamps: false })) throw new Error("Session unavailable");
  return login;
}

// Use inside the same transaction as the protected database operation. The
// caller supplies the user ID derived from trusted server configuration; a
// pending device can still pair but must not use this authorization gate.
export async function activeAuthorizedMatrixDevice(session, accountId, sessionId, userId, deviceId) {
  return activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["authorized"]);
}
export async function activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, states) {
  validate(accountId, sessionId, userId, deviceId);
  if (!Array.isArray(states) || states.length === 0 || states.some(state => !["authorized", "pending"].includes(state))) throw new Error("Invalid device access");
  const login = await activeMatrixLogin(session, accountId, sessionId);
  if (login.matrixDevice?.userId !== userId || login.matrixDevice?.deviceId !== deviceId) throw new Error("Session is not bound to device");
  const device = await MatrixDevice.findOneAndUpdate({ userId, deviceId, serverAuthorization: { $in: states }, revokedAt: null }, { $inc: { revision: 1 } }, { session, new: true });
  if (!device) throw new Error("Authorized device unavailable");
  return device;
}
export async function issueMatrixBindingChallenge(accountId, sessionId, userId, deviceId) {
  validate(accountId, sessionId, userId, deviceId);
  const token = randomBytes(32).toString("base64url"), expiresAt = new Date(Date.now() + 120000);
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    if (login.matrixDevice?.deviceId && (login.matrixDevice.userId !== userId || login.matrixDevice.deviceId !== deviceId)) throw new Error("Session already bound");
    const device = await MatrixDevice.findOneAndUpdate({ userId, deviceId, revokedAt: null }, { $inc: { revision: 1 } }, { session });
    if (!device) throw new Error("Device unavailable");
    const count = await MatrixBindingChallenge.countDocuments({ sessionId, consumedAt: null, expiresAt: { $gt: new Date() } }).session(session);
    if (count >= 5) throw new Error("Too many binding challenges");
    await MatrixBindingChallenge.create([{ tokenHash: hash(token), sessionId, accountId, userId, deviceId, expiresAt }], { session });
  });
  return { token, message: bindingMessage(userId, deviceId, sessionId, token), expiresAt };
}
export async function bindMatrixDevice(accountId, sessionId, userId, deviceId, token, signature) {
  validate(accountId, sessionId, userId, deviceId);
  if (typeof token !== "string" || !TOKEN.test(token)) throw new Error("Invalid binding challenge");
  const selector = { tokenHash: hash(token), sessionId, accountId, userId, deviceId, consumedAt: null, failures: { $lt: 5 }, expiresAt: { $gt: new Date() } };
  const challenge = await MatrixBindingChallenge.findOne(selector).lean();
  if (!challenge) throw new Error("Binding challenge unavailable");
  const device = await MatrixDevice.findOne({ userId, deviceId, revokedAt: null }).lean();
  if (!device || !verifyMatrixEd25519(device.deviceKeys.keys[`ed25519:${deviceId}`], signature, bindingMessage(userId, deviceId, sessionId, token))) {
    await MatrixBindingChallenge.updateOne(selector, { $inc: { failures: 1 } });
    throw new Error("Invalid device signature");
  }
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    if (login.matrixDevice?.deviceId && (login.matrixDevice.userId !== userId || login.matrixDevice.deviceId !== deviceId)) throw new Error("Session already bound");
    const current = await MatrixDevice.findOneAndUpdate({ userId, deviceId, revokedAt: null }, { $inc: { revision: 1 } }, { session, new: true });
    if (!current || current.deviceKeys.keys[`ed25519:${deviceId}`] !== device.deviceKeys.keys[`ed25519:${deviceId}`]) throw new Error("Device unavailable");
    const used = await MatrixBindingChallenge.findOneAndUpdate(selector, { $set: { consumedAt: new Date() } }, { session, new: true });
    if (!used) throw new Error("Binding challenge unavailable");
    await LoginSession.updateOne({ _id: sessionId, userId: accountId }, { $set: { matrixDevice: { userId, deviceId } } }, { session });
  });
  return { userId, deviceId };
}
export async function boundMatrixDevice(accountId, sessionId) {
  if (typeof accountId !== "string" || !ACCOUNT.test(accountId) || typeof sessionId !== "string" || !SESSION.test(sessionId)) throw new Error("Invalid session");
  const login = await LoginSession.findOne({ _id: sessionId, userId: accountId, expiresAt: { $gt: new Date() } }).lean();
  const version = login?.sessionVersion === 0 ? { $or: [{ sessionVersion: 0 }, { sessionVersion: { $exists: false } }] } : { sessionVersion: login?.sessionVersion };
  if (!login || !await User.exists({ _id: accountId, ...version }) || !login.matrixDevice?.deviceId) throw new Error("Session unavailable");
  const { userId, deviceId } = login.matrixDevice;
  validate(accountId, sessionId, userId, deviceId);
  if (!await MatrixDevice.exists({ userId, deviceId, revokedAt: null })) throw new Error("Device unavailable");
  return { userId, deviceId };
}
