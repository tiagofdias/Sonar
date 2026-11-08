// Opt-in experimental device registration, snapshots and same-account
// verification transport, and explicit legacy-key first-device bootstrap.
import express from "express";
import { protectRoute } from "../middleware/auth.middleware.js";
import { createAuthRateLimit } from "../middleware/authRateLimit.js";
import { beginMatrixEnrollment, completeMatrixEnrollment, listMatrixAccountDevices, authorizeMatrixDevice } from "../lib/matrixEnrollment.js";
import { issueMatrixBindingChallenge, bindMatrixDevice } from "../lib/matrixSessionBinding.js";
import { sessionMatrixRoomSnapshot } from "../lib/matrixRooms.js";
import { uploadSessionOneTimeKeys, querySessionOwnDeviceKeys, claimSessionOwnOneTimeKey, revokeSessionMatrixDevice } from "../lib/matrixKeys.js";
import { enqueueSessionVerification, readSessionVerification, acknowledgeSessionVerification } from "../lib/matrixToDevice.js";
import { beginLegacyMatrixBootstrap, completeLegacyMatrixBootstrap } from "../lib/matrixBootstrap.js";

export function createMatrixDeviceRouter(serverName) {
  if (typeof serverName !== "string" || !/^[a-z0-9](?:[a-z0-9.-]{0,198}[a-z0-9])?$/.test(serverName) || serverName.includes("..")) throw new Error("Invalid MATRIX_SERVER_NAME");
  const router = express.Router();
  router.use(protectRoute);
  router.use(createAuthRateLimit({ limit: 60, windowMs: 60000 }));
  router.use((req, res, next) => {
    res.set("Cache-Control", "no-store");
    if (!req.sessionId) return res.status(401).json({ message: "Sign in again to use this experimental feature." });
    next();
  });
  const identity = req => ({ accountId: String(req.user._id), sessionId: req.sessionId, userId: `@${req.user._id}:${serverName}` });
  const safe = fn => async (req, res) => {
    try { res.json(await fn(req, identity(req))); }
    catch { res.status(400).json({ message: "Device operation failed. Check the device and try again." }); }
  };
  router.get("/identity", (req, res) => res.json({ userId: identity(req).userId }));
  router.get("/list", safe((_req, id) => listMatrixAccountDevices(id.accountId, id.sessionId, serverName)));
  router.get("/room", safe((req, id) => sessionMatrixRoomSnapshot(id.accountId, id.sessionId, serverName, req.query.roomId, req.query.deviceId)));
  router.post("/enroll/challenge", safe((req, id) => beginMatrixEnrollment(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.deviceKeys)));
  router.post("/enroll/complete", safe((req, id) => completeMatrixEnrollment(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.token, req.body?.signature)));
  router.post("/bind/challenge", safe((req, id) => issueMatrixBindingChallenge(id.accountId, id.sessionId, id.userId, req.body?.deviceId)));
  router.post("/bind/complete", safe((req, id) => bindMatrixDevice(id.accountId, id.sessionId, id.userId, req.body?.deviceId, req.body?.token, req.body?.signature)));
  router.post("/bootstrap/legacy/challenge", safe((req, id) => beginLegacyMatrixBootstrap(id.accountId, id.sessionId, serverName, req.body?.deviceId)));
  router.post("/bootstrap/legacy/complete", safe((req, id) => completeLegacyMatrixBootstrap(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.token, req.body?.proof)));
  router.post("/keys/upload", safe((req, id) => uploadSessionOneTimeKeys(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.oneTimeKeys)));
  router.get("/keys/own", safe((req, id) => querySessionOwnDeviceKeys(id.accountId, id.sessionId, serverName, req.query.deviceId)));
  router.post("/keys/claim-own", safe((req, id) => claimSessionOwnOneTimeKey(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.targetDeviceId, req.body?.algorithm)));
  // Payloads are opaque to the server. Scope is limited to another device of
  // this account; the SDK must verify the SAS before a client grants trust.
  router.post("/verification/send", safe(async (req, id) => {
    await enqueueSessionVerification(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.transactionId, req.body?.eventType, req.body?.messages);
    return { accepted: true };
  }));
  router.get("/verification/inbox", safe(async (req, id) => ({ events: await readSessionVerification(id.accountId, id.sessionId, serverName, req.query.deviceId) })));
  router.post("/verification/ack", safe(async (req, id) => {
    await acknowledgeSessionVerification(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.ids);
    return { acknowledged: true };
  }));
  // The server checks account, binding and signing key; only the clients can
  // know whether the humans actually compared the end-to-end SAS codes.
  router.post("/authorize", safe(async (req, id) => {
    await authorizeMatrixDevice(id.accountId, id.sessionId, serverName, req.body?.deviceId, req.body?.targetDeviceId, req.body?.expectedEd25519);
    return { authorized: true, deviceId: req.body.targetDeviceId };
  }));
  router.post("/revoke", safe((req, id) => {
    const { deviceId, targetDeviceId, expectedEd25519 } = req.body || {};
    if (deviceId === targetDeviceId || typeof expectedEd25519 !== "string") throw new Error("Invalid target device");
    return revokeSessionMatrixDevice(id.accountId, id.sessionId, serverName, deviceId, targetDeviceId, expectedEd25519);
  }));
  return router;
}
