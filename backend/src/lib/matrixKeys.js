// Internal Matrix key-directory primitives. These are intentionally not exposed
// over HTTP until session-to-device binding and device verification exist.
import mongoose from "mongoose";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixOneTimeKey from "../models/matrixOneTimeKey.model.js";
import MatrixToDevice from "../models/matrixToDevice.model.js";
import MatrixEventTransaction from "../models/matrixEventTransaction.model.js";
import MatrixBindingChallenge from "../models/matrixBindingChallenge.model.js";
import MatrixRoom from "../models/matrixRoom.model.js";
import LoginSession from "../models/loginSession.model.js";
import { verifyMatrixEd25519 } from "./matrixSignatures.js";
import { activeBoundMatrixDevice, activeAuthorizedMatrixDevice, matrixSessionIdentity } from "./matrixSessionBinding.js";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const OTK = /^signed_curve25519:[A-Za-z0-9+/=_-]{1,80}$/;
const validIdentity = (userId, deviceId) => typeof userId === "string" && USER.test(userId) && typeof deviceId === "string" && DEVICE.test(deviceId);
const object = value => value !== null && typeof value === "object" && !Array.isArray(value);
const exactKeys = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
function base64(value, length) {
  if (typeof value !== "string" || !/^[A-Za-z0-9+/]+$/.test(value)) return false;
  const bytes = Buffer.from(value, "base64");
  return bytes.length === length && bytes.toString("base64").replace(/=+$/, "") === value;
}
function signature(signatures, userId, deviceId) {
  const key = `ed25519:${deviceId}`;
  if (!exactKeys(signatures, [userId]) || !exactKeys(signatures[userId], [key]) || !base64(signatures[userId][key], 64)) throw new Error("Invalid signatures");
  return { [userId]: { [key]: signatures[userId][key] } };
}
export function signedDevice(value, userId, deviceId) {
  if (!validIdentity(userId, deviceId) || !exactKeys(value, ["user_id", "device_id", "algorithms", "keys", "signatures"]) || value.user_id !== userId || value.device_id !== deviceId) throw new Error("Invalid device identity");
  if (!Array.isArray(value.algorithms) || value.algorithms.length !== 2 || new Set(value.algorithms).size !== 2 || !["m.olm.v1.curve25519-aes-sha2", "m.megolm.v1.aes-sha2"].every(item => value.algorithms.includes(item))) throw new Error("Invalid algorithms");
  const curve = `curve25519:${deviceId}`, ed = `ed25519:${deviceId}`;
  if (!exactKeys(value.keys, [curve, ed]) || Object.keys(value.keys).length !== 2 || !base64(value.keys[curve], 32) || !base64(value.keys[ed], 32)) throw new Error("Invalid public keys");
  const signatures = signature(value.signatures, userId, deviceId);
  // All permitted values and keys are ASCII. This fixed-key serialization is
  // Matrix canonical JSON for this strictly validated device-key shape.
  const canonical = JSON.stringify({ algorithms: value.algorithms, device_id: deviceId, keys: { [curve]: value.keys[curve], [ed]: value.keys[ed] }, user_id: userId });
  if (!verifyMatrixEd25519(value.keys[ed], signatures[userId][ed], canonical)) throw new Error("Invalid device signature");
  return { user_id: userId, device_id: deviceId, algorithms: [...value.algorithms], keys: { [curve]: value.keys[curve], [ed]: value.keys[ed] }, signatures };
}
function signedOneTime(value, userId, deviceId) {
  if (!exactKeys(value, ["key", "signatures"]) || !base64(value.key, 32)) throw new Error("Invalid one-time key");
  return { key: value.key, signatures: signature(value.signatures, userId, deviceId) };
}

export async function createDevice(userId, deviceId, keys) {
  const deviceKeys = signedDevice(keys, userId, deviceId);
  return MatrixDevice.create({ userId, deviceId, deviceKeys });
}

export async function uploadOneTimeKeys(userId, deviceId, input) {
  return uploadKeys(userId, deviceId, input, null);
}
export async function uploadSessionOneTimeKeys(accountId, sessionId, serverName, deviceId, input) {
  return uploadKeys(matrixSessionIdentity(accountId, sessionId, serverName, deviceId), deviceId, input, { accountId, sessionId });
}
async function uploadKeys(userId, deviceId, input, auth) {
  if (!validIdentity(userId, deviceId) || !object(input) || Object.keys(input).length > 100) throw new Error("Invalid key upload");
  const keys = Object.entries(input).map(([keyId, value]) => {
    if (!OTK.test(keyId)) throw new Error("Invalid one-time key id");
    return { keyId, signedKey: signedOneTime(value, userId, deviceId) };
  });
  let count = 0;
  await mongoose.connection.transaction(async session => {
    // Serializes concurrent uploads and ensures the device is still registered.
    const device = auth ? await activeBoundMatrixDevice(session, auth.accountId, auth.sessionId, userId, deviceId, ["authorized", "pending"]) : await MatrixDevice.findOneAndUpdate({ userId, deviceId, revokedAt: null }, { $inc: { revision: 1 } }, { session, new: true });
    if (!device) throw new Error("Unknown device");
    const signingKey = device.deviceKeys.keys[`ed25519:${deviceId}`];
    for (const item of keys) {
      const signed = item.signedKey;
      if (!verifyMatrixEd25519(signingKey, signed.signatures[userId][`ed25519:${deviceId}`], JSON.stringify({ key: signed.key }))) throw new Error("Invalid one-time key signature");
    }
    const existing = await MatrixOneTimeKey.find({ userId, deviceId, keyId: { $in: keys.map(key => key.keyId) } }).session(session).lean();
    const seen = new Map(existing.map(key => [key.keyId, key.signedKey]));
    for (const item of keys) if (seen.has(item.keyId) && JSON.stringify(seen.get(item.keyId)) !== JSON.stringify(item.signedKey)) throw new Error("One-time key cannot be replaced");
    const additions = keys.filter(item => !seen.has(item.keyId));
    const retained = await MatrixOneTimeKey.countDocuments({ userId, deviceId }).session(session);
    if (retained + additions.length > 100000) throw new Error("Device key history capacity reached");
    count = await MatrixOneTimeKey.countDocuments({ userId, deviceId, claimedAt: null }).session(session);
    if (count + additions.length > 1000) throw new Error("Too many one-time keys");
    if (additions.length) await MatrixOneTimeKey.insertMany(additions.map(item => ({ userId, deviceId, ...item })), { session });
    count += additions.length;
  });
  return { one_time_key_counts: { signed_curve25519: count } };
}

// Pairing lookup is scoped to the caller's own account and remains separate
// from the room-scoped directory required for messages to other accounts.
export async function querySessionOwnDeviceKeys(accountId, sessionId, serverName, deviceId) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  let rows;
  await mongoose.connection.transaction(async session => {
    await activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["authorized", "pending"]);
    rows = await MatrixDevice.find({ userId, revokedAt: null, serverAuthorization: { $in: ["authorized", "pending"] } }).session(session).lean();
  });
  return { device_keys: { [userId]: Object.fromEntries(rows.map(row => [row.deviceId, row.deviceKeys])) }, failures: {} };
}

export async function claimSessionOwnOneTimeKey(accountId, sessionId, serverName, deviceId, targetId, algorithm) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (!validIdentity(userId, targetId) || targetId === deviceId || algorithm !== "signed_curve25519") throw new Error("Invalid key claim");
  let key;
  await mongoose.connection.transaction(async session => {
    const sender = await activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["authorized", "pending"]);
    const target = await MatrixDevice.findOneAndUpdate({ userId, deviceId: targetId, revokedAt: null, serverAuthorization: { $in: ["authorized", "pending"] } }, { $inc: { revision: 1 } }, { session, new: true });
    if (!target || (sender.serverAuthorization !== "authorized" && target.serverAuthorization !== "authorized")) throw new Error("Authorized device required");
    key = await MatrixOneTimeKey.findOneAndUpdate({ userId, deviceId: targetId, claimedAt: null }, { $set: { claimedAt: new Date() } }, { sort: { createdAt: 1, _id: 1 }, session, new: true }).lean();
  });
  return { one_time_keys: key ? { [userId]: { [targetId]: { [key.keyId]: key.signedKey } } } : {}, failures: {} };
}

// Only approved devices belonging to current members are returned; room
// revision binds the lookup to the recipient list used for the message.
export async function querySessionRoomDeviceKeys(accountId, sessionId, serverName, deviceId, roomId, revision, users) {
  const sender = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof roomId !== "string" || !/^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/.test(roomId) || !Number.isSafeInteger(revision) || revision < 0 || !Array.isArray(users) || users.length < 1 || users.length > 32 || new Set(users).size !== users.length || users.some(user => typeof user !== "string" || !USER.test(user))) throw new Error("Invalid room key query");
  let rows;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, revision, members: { $all: [sender, ...users] } }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room membership changed");
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, sender, deviceId);
    rows = await MatrixDevice.find({ userId: { $in: users }, revokedAt: null, serverAuthorization: "authorized" }).session(session).lean();
  });
  const device_keys = Object.fromEntries(users.map(user => [user, {}]));
  for (const row of rows) device_keys[row.userId][row.deviceId] = row.deviceKeys;
  return { device_keys, failures: {} };
}

export async function claimSessionRoomOneTimeKey(accountId, sessionId, serverName, deviceId, roomId, revision, targetUser, targetDevice, algorithm) {
  const sender = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof roomId !== "string" || !/^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/.test(roomId) || !Number.isSafeInteger(revision) || revision < 0 || !validIdentity(targetUser, targetDevice) || algorithm !== "signed_curve25519" || (targetUser === sender && targetDevice === deviceId)) throw new Error("Invalid room key claim");
  let key;
  await mongoose.connection.transaction(async session => {
    const room = await MatrixRoom.findOneAndUpdate({ roomId, revision, members: { $all: [sender, targetUser] } }, { $inc: { activityVersion: 1 } }, { session });
    if (!room) throw new Error("Room membership changed");
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, sender, deviceId);
    const target = await MatrixDevice.findOneAndUpdate({ userId: targetUser, deviceId: targetDevice, revokedAt: null, serverAuthorization: "authorized" }, { $inc: { revision: 1 } }, { session });
    if (!target) throw new Error("Target device unavailable");
    key = await MatrixOneTimeKey.findOneAndUpdate({ userId: targetUser, deviceId: targetDevice, claimedAt: null }, { $set: { claimedAt: new Date() } }, { sort: { createdAt: 1, _id: 1 }, session, new: true }).lean();
  });
  return { one_time_keys: key ? { [targetUser]: { [targetDevice]: { [key.keyId]: key.signedKey } } } : {}, failures: {} };
}

export async function queryDeviceKeys(userIds) {
  if (!Array.isArray(userIds) || userIds.length > 100 || userIds.some(value => typeof value !== "string" || !USER.test(value))) throw new Error("Invalid key query");
  const rows = await MatrixDevice.find({ userId: { $in: userIds }, revokedAt: null }).lean();
  const device_keys = Object.fromEntries(userIds.map(userId => [userId, {}]));
  for (const row of rows) device_keys[row.userId][row.deviceId] = row.deviceKeys;
  return { device_keys, failures: {} };
}

export async function claimOneTimeKey(userId, deviceId, algorithm) {
  if (!validIdentity(userId, deviceId) || algorithm !== "signed_curve25519") throw new Error("Invalid key claim");
  const key = await MatrixOneTimeKey.findOneAndUpdate({ userId, deviceId, claimedAt: null }, { $set: { claimedAt: new Date() } }, { sort: { createdAt: 1, _id: 1 }, new: true }).lean();
  return { one_time_keys: key ? { [userId]: { [deviceId]: { [key.keyId]: key.signedKey } } } : {}, failures: {} };
}

export async function revokeDevice(userId, deviceId) {
  if (!validIdentity(userId, deviceId)) throw new Error("Invalid device");
  let revoked = false;
  await mongoose.connection.transaction(async session => {
    revoked = await revokeInTransaction(session, userId, deviceId);
  });
  return revoked;
}

async function revokeInTransaction(session, userId, deviceId, expectedEd25519) {
  const selector = { userId, deviceId, revokedAt: null };
  if (expectedEd25519 !== undefined) selector[`deviceKeys.keys.ed25519:${deviceId}`] = expectedEd25519;
  const device = await MatrixDevice.findOneAndUpdate(selector, { $set: { revokedAt: new Date() }, $inc: { revision: 1 } }, { session, new: true });
  if (!device) return false;
  // Any existing Megolm session may already have reached this device. In the
  // same transaction, invalidate the room revision so future sends must rotate.
  if (device.serverAuthorization === "authorized") await MatrixRoom.updateMany({ members: userId }, { $inc: { revision: 1 } }, { session });
  await MatrixOneTimeKey.deleteMany({ userId, deviceId }, { session });
  await MatrixToDevice.deleteMany({ recipient: userId, deviceId }, { session });
  await MatrixEventTransaction.deleteMany({ sender: userId, senderDevice: deviceId }, { session });
  await MatrixBindingChallenge.deleteMany({ userId, deviceId }, { session });
  await LoginSession.updateMany({ "matrixDevice.userId": userId, "matrixDevice.deviceId": deviceId }, { $unset: { matrixDevice: 1 } }, { session });
  return true;
}

// No HTTP endpoint should invoke the unrestricted revokeDevice primitive.
// Device access and session revocation serialize with this transaction.
export async function revokeSessionMatrixDevice(accountId, sessionId, serverName, actorId, targetId, expectedEd25519) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, actorId);
  if (!validIdentity(userId, targetId) || (expectedEd25519 !== undefined && (typeof expectedEd25519 !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(expectedEd25519)))) throw new Error("Invalid device");
  let revoked;
  await mongoose.connection.transaction(async session => {
    await activeAuthorizedMatrixDevice(session, accountId, sessionId, userId, actorId);
    revoked = await revokeInTransaction(session, userId, targetId, expectedEd25519);
    if (!revoked) throw new Error("Device unavailable");
  });
  return { deviceId: targetId, revoked: true };
}
