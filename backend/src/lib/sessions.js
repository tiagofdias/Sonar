import { createHash, randomBytes } from "node:crypto";
import LoginSession from "../models/loginSession.model.js";
export const SESSION_MS = 7 * 24 * 60 * 60 * 1000;
export const sessionHash = (id) => createHash("sha256").update(id).digest("hex");
export async function createSession(userId, sessionVersion, method) {
  const tokenId = randomBytes(32).toString("hex");
  await LoginSession.create({ _id: sessionHash(tokenId), userId, sessionVersion, method, expiresAt: new Date(Date.now() + SESSION_MS) });
  return tokenId;
}
export async function validSession(payload) {
  // Older signed cookies have no session ID and remain usable until expiry or global revocation.
  if (payload.sessionId === undefined) return true;
  if (typeof payload.sessionId !== "string" || !/^[a-f0-9]{64}$/.test(payload.sessionId)) return false;
  return Boolean(await LoginSession.exists({ _id: sessionHash(payload.sessionId), userId: payload.userId, sessionVersion: payload.sessionVersion || 0, expiresAt: { $gt: new Date() } }));
}
