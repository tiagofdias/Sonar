// Internal enrollment/authorization primitives. Server authorization does not
// establish cryptographic trust: clients must independently complete Matrix SAS
// verification before approving, sending keys, or treating a device as trusted.
// A gateway must derive serverName from trusted operator configuration and must
// make first-device bootstrap an explicit user action on the existing device.
import { createHash, randomBytes } from "node:crypto";
import mongoose from "mongoose";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixAccount from "../models/matrixAccount.model.js";
import MatrixEnrollment from "../models/matrixEnrollment.model.js";
import MatrixRoom from "../models/matrixRoom.model.js";
import { signedDevice } from "./matrixKeys.js";
import { activeMatrixLogin, activeAuthorizedMatrixDevice, activeBoundMatrixDevice, matrixSessionIdentity } from "./matrixSessionBinding.js";
import { verifyMatrixEd25519 } from "./matrixSignatures.js";

const ACCOUNT = /^[a-f0-9]{24}$/, SESSION = /^[a-f0-9]{64}$/, DEVICE = /^[A-Za-z0-9]{1,32}$/, DOMAIN = /^[a-z0-9](?:[a-z0-9.-]{0,198}[a-z0-9])?$/, TOKEN = /^[A-Za-z0-9_-]{43}$/;
const hash = value => createHash("sha256").update(value).digest("hex");
function identity(accountId, sessionId, serverName, deviceId) {
  if (typeof accountId !== "string" || !ACCOUNT.test(accountId) || typeof sessionId !== "string" || !SESSION.test(sessionId) || typeof serverName !== "string" || !DOMAIN.test(serverName) || serverName.includes("..") || typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid enrollment identity");
  return `@${accountId}:${serverName}`;
}
function message(userId, deviceId, sessionId, keysHash, token) { return `sonar-matrix-device-enrollment-v1\n${userId}\n${deviceId}\n${sessionId}\n${keysHash}\n${token}`; }

// Server authorization is only an access state. Clients must separately show
// their own cryptographic verification status and never equate the two.
export async function listMatrixAccountDevices(accountId, sessionId, serverName) {
  let result;
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    const deviceId = login.matrixDevice?.deviceId;
    const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
    if (login.matrixDevice?.userId !== userId) throw new Error("Session is not bound to device");
    await activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["authorized", "pending"]);
    const devices = await MatrixDevice.find({ userId, revokedAt: null, serverAuthorization: { $in: ["authorized", "pending"] } }).sort({ createdAt: 1, deviceId: 1 }).limit(100).session(session).lean();
    result = devices.map(device => ({ deviceId: device.deviceId, serverAuthorization: device.serverAuthorization, ed25519: device.deviceKeys.keys[`ed25519:${device.deviceId}`], current: device.deviceId === deviceId, createdAt: device.createdAt }));
  });
  return { devices: result };
}

export async function beginMatrixEnrollment(accountId, sessionId, serverName, deviceId, input) {
  const userId = identity(accountId, sessionId, serverName, deviceId);
  const deviceKeys = signedDevice(input, userId, deviceId);
  const keysHash = hash(JSON.stringify(deviceKeys));
  const token = randomBytes(32).toString("base64url"), expiresAt = new Date(Date.now() + 120000);
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    if (login.matrixDevice?.deviceId) throw new Error("Session already bound");
    if (await MatrixDevice.exists({ userId, deviceId }).session(session)) throw new Error("Device ID already registered");
    if (await MatrixEnrollment.countDocuments({ sessionId, consumedAt: null, expiresAt: { $gt: new Date() } }).session(session) >= 5) throw new Error("Too many enrollment challenges");
    await MatrixEnrollment.create([{ tokenHash: hash(token), accountId, sessionId, userId, deviceId, deviceKeys, keysHash, expiresAt }], { session });
  });
  return { token, message: message(userId, deviceId, sessionId, keysHash, token), expiresAt };
}

export async function completeMatrixEnrollment(accountId, sessionId, serverName, deviceId, token, signature) {
  const userId = identity(accountId, sessionId, serverName, deviceId);
  if (typeof token !== "string" || !TOKEN.test(token)) throw new Error("Invalid enrollment challenge");
  const selector = { tokenHash: hash(token), accountId, sessionId, userId, deviceId, consumedAt: null, failures: { $lt: 5 }, expiresAt: { $gt: new Date() } };
  const challenge = await MatrixEnrollment.findOne(selector).lean();
  if (!challenge) throw new Error("Enrollment challenge unavailable");
  if (hash(JSON.stringify(challenge.deviceKeys)) !== challenge.keysHash) throw new Error("Enrollment challenge corrupted");
  const publicKey = challenge.deviceKeys.keys[`ed25519:${deviceId}`];
  if (!verifyMatrixEd25519(publicKey, signature, message(userId, deviceId, sessionId, challenge.keysHash, token))) {
    await MatrixEnrollment.updateOne(selector, { $inc: { failures: 1 } });
    throw new Error("Invalid enrollment signature");
  }
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    if (login.matrixDevice?.deviceId) throw new Error("Session already bound");
    const consumed = await MatrixEnrollment.findOneAndUpdate(selector, { $set: { consumedAt: new Date() } }, { session, new: true });
    if (!consumed) throw new Error("Enrollment challenge unavailable");
    if (consumed.keysHash !== challenge.keysHash || hash(JSON.stringify(consumed.deviceKeys)) !== challenge.keysHash) throw new Error("Enrollment challenge changed");
    // No device is implicitly trusted or authorized by registration.
    await MatrixAccount.findOneAndUpdate({ _id: userId }, { $inc: { revision: 1 }, $setOnInsert: { bootstrapped: false } }, { session, upsert: true });
    if (await MatrixDevice.countDocuments({ userId }).session(session) >= 100) throw new Error("Device capacity reached");
    await MatrixDevice.create([{ userId, deviceId, deviceKeys: consumed.deviceKeys, serverAuthorization: "pending" }], { session });
  });
  return { userId, deviceId, serverAuthorization: "pending" };
}

// Only for an explicit first-device action, never an automatic login fallback.
export async function bootstrapMatrixDevice(accountId, sessionId, serverName, deviceId) {
  const userId = identity(accountId, sessionId, serverName, deviceId);
  await mongoose.connection.transaction(async session => {
    const login = await activeMatrixLogin(session, accountId, sessionId);
    if (login.matrixDevice?.userId !== userId || login.matrixDevice?.deviceId !== deviceId) throw new Error("Session is not bound to device");
    const state = await MatrixAccount.findOneAndUpdate({ _id: userId, bootstrapped: false }, { $set: { bootstrapped: true }, $inc: { revision: 1 } }, { session, new: true });
    if (!state) throw new Error("Account already bootstrapped");
    const target = await MatrixDevice.findOneAndUpdate({ userId, deviceId, serverAuthorization: "pending", revokedAt: null }, { $set: { serverAuthorization: "authorized", authorizedAt: new Date(), authorizedBy: deviceId }, $inc: { revision: 1 } }, { session, new: true });
    if (!target) throw new Error("Device unavailable");
    await MatrixRoom.updateMany({ members: userId }, { $inc: { revision: 1 } }, { session });
  });
}

// Caller must first check Matrix SDK SAS verification on the approving client.
// This server-side access control alone cannot prove that SAS was compared.
export async function authorizeMatrixDevice(accountId, sessionId, serverName, approverId, targetId, expectedEd25519) {
  const userId = identity(accountId, sessionId, serverName, approverId);
  if (typeof targetId !== "string" || !DEVICE.test(targetId) || targetId === approverId || typeof expectedEd25519 !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(expectedEd25519)) throw new Error("Invalid target device");
  await mongoose.connection.transaction(async session => {
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, userId, approverId);
    const target = await MatrixDevice.findOneAndUpdate({ userId, deviceId: targetId, serverAuthorization: "pending", revokedAt: null, [`deviceKeys.keys.ed25519:${targetId}`]: expectedEd25519 }, { $set: { serverAuthorization: "authorized", authorizedAt: new Date(), authorizedBy: approverId }, $inc: { revision: 1 } }, { session, new: true });
    if (!target) throw new Error("Target unavailable");
    await MatrixRoom.updateMany({ members: userId }, { $inc: { revision: 1 } }, { session });
  });
}
