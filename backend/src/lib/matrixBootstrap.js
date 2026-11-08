// Explicit first-device authorization using possession of the existing browser
// ECDH key. This is not a Matrix verification or a messaging protocol.
import { createCipheriv, createECDH, createHash, randomBytes, timingSafeEqual } from "node:crypto";
import mongoose from "mongoose";
import User from "../models/user.model.js";
import MatrixAccount from "../models/matrixAccount.model.js";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixRoom from "../models/matrixRoom.model.js";
import Challenge from "../models/matrixBootstrapChallenge.model.js";
import { activeBoundMatrixDevice, matrixSessionIdentity } from "./matrixSessionBinding.js";

const hash = value => createHash("sha256").update(value).digest("hex");
const TOKEN = /^[A-Za-z0-9_-]{43}$/;
const messageFor = (userId, deviceId, sessionId, token) => `sonar-legacy-bootstrap-v1\n${userId}\n${deviceId}\n${sessionId}\n${token}`;
function publicKey(text) {
  if (typeof text !== "string" || !/^[A-Za-z0-9+/]{87}=$/.test(text)) throw new Error("Legacy key unavailable");
  const bytes = Buffer.from(text, "base64");
  if (bytes.length !== 65 || bytes[0] !== 4 || bytes.toString("base64") !== text) throw new Error("Invalid legacy key");
  return bytes;
}
export async function beginLegacyMatrixBootstrap(accountId, sessionId, serverName, deviceId) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  const token = randomBytes(32).toString("base64url");
  let result;
  await mongoose.connection.transaction(async session => {
    await activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["pending"]);
    if (!await MatrixAccount.exists({ _id: userId, bootstrapped: false }).session(session)) throw new Error("Account already bootstrapped");
    if (await Challenge.countDocuments({ sessionId, consumedAt: null, expiresAt: { $gt: new Date() } }).session(session) >= 5) throw new Error("Too many challenges");
    const user = await User.findById(accountId).select("encryptionKey").session(session).lean();
    const legacyPublicKey = user?.encryptionKey;
    const ecdh = createECDH("prime256v1");
    ecdh.generateKeys();
    const shared = ecdh.computeSecret(publicKey(legacyPublicKey));
    const proof = randomBytes(32), iv = randomBytes(12), expiresAt = new Date(Date.now() + 120000);
    const message = messageFor(userId, deviceId, sessionId, token);
    try {
      const cipher = createCipheriv("aes-256-gcm", shared, iv);
      cipher.setAAD(Buffer.from(message));
      const ciphertext = Buffer.concat([cipher.update(proof), cipher.final(), cipher.getAuthTag()]);
      await Challenge.create([{ tokenHash: hash(token), proofHash: hash(proof), legacyKeyHash: hash(legacyPublicKey), accountId, sessionId, userId, deviceId, expiresAt }], { session });
      result = { token, message, legacyPublicKey, serverPublicKey: ecdh.getPublicKey().toString("base64"), iv: iv.toString("base64"), ciphertext: ciphertext.toString("base64"), expiresAt };
    } finally { shared.fill(0); proof.fill(0); }
  });
  return result;
}
export async function completeLegacyMatrixBootstrap(accountId, sessionId, serverName, deviceId, token, proofText) {
  const userId = matrixSessionIdentity(accountId, sessionId, serverName, deviceId);
  if (typeof token !== "string" || !TOKEN.test(token) || typeof proofText !== "string" || !TOKEN.test(proofText)) throw new Error("Invalid proof");
  const proof = Buffer.from(proofText, "base64url");
  if (proof.length !== 32 || proof.toString("base64url") !== proofText) throw new Error("Invalid proof");
  const selector = { tokenHash: hash(token), accountId, sessionId, userId, deviceId, consumedAt: null, failures: { $lt: 5 }, expiresAt: { $gt: new Date() } };
  const challenge = await Challenge.findOne(selector).lean();
  const digest = Buffer.from(hash(proof), "hex"); proof.fill(0);
  if (!challenge || !timingSafeEqual(digest, Buffer.from(challenge.proofHash, "hex"))) {
    await Challenge.updateOne(selector, { $inc: { failures: 1 } });
    throw new Error("Invalid proof");
  }
  await mongoose.connection.transaction(async session => {
    await activeBoundMatrixDevice(session, accountId, sessionId, userId, deviceId, ["pending"]);
    const user = await User.findById(accountId).select("encryptionKey").session(session).lean();
    if (typeof user?.encryptionKey !== "string" || hash(user.encryptionKey) !== challenge.legacyKeyHash) throw new Error("Legacy key changed");
    const used = await Challenge.findOneAndUpdate({ ...selector, proofHash: challenge.proofHash, legacyKeyHash: challenge.legacyKeyHash, expiresAt: { $gt: new Date() } }, { $set: { consumedAt: new Date() } }, { session, new: true });
    if (!used) throw new Error("Challenge unavailable");
    const state = await MatrixAccount.findOneAndUpdate({ _id: userId, bootstrapped: false }, { $set: { bootstrapped: true }, $inc: { revision: 1 } }, { session, new: true });
    if (!state) throw new Error("Account already bootstrapped");
    const target = await MatrixDevice.findOneAndUpdate({ userId, deviceId, serverAuthorization: "pending", revokedAt: null }, { $set: { serverAuthorization: "authorized", authorizedAt: new Date(), authorizedBy: deviceId }, $inc: { revision: 1 } }, { session, new: true });
    if (!target) throw new Error("Device unavailable");
    await MatrixRoom.updateMany({ members: userId }, { $inc: { revision: 1 } }, { session });
  });
  return { authorized: true, deviceId };
}
