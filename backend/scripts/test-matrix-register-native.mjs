// Uses synthetic credentials and an isolated in-memory replica set only.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import express from "express";
import cookieParser from "cookie-parser";
import bcrypt from "bcryptjs";
import mongoose from "mongoose";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import User from "../src/models/user.model.js";
import MatrixDevice from "../src/models/matrixDevice.model.js";
import MatrixRoom from "../src/models/matrixRoom.model.js";
import MatrixRoomEvent from "../src/models/matrixRoomEvent.model.js";
import LoginSession from "../src/models/loginSession.model.js";
import { bootstrapMatrixDevice } from "../src/lib/matrixEnrollment.js";
import { createMatrixRoom } from "../src/lib/matrixRooms.js";
import authRoutes from "../src/routes/auth.route.js";
import { createMatrixDeviceRouter } from "../src/routes/matrixDevice.route.js";
import { createMatrixMessagingRouter } from "../src/routes/matrixMessaging.route.js";
import { readSessionContactVerification } from "../src/lib/matrixToDevice.js";

process.env.JWT_SECRET = "synthetic-native-matrix-registration-secret";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const temp = await mkdtemp(resolve(tmpdir(), "sonar-native-register-"));
const db = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
let server, child;
try {
  await mongoose.connect(db.getUri("sonar-native-matrix-registration"));
  await MatrixRoomEvent.init();
  assert.ok((await MatrixRoomEvent.collection.indexes()).some(index => index.key.expiresAt === 1 && index.expireAfterSeconds === 0), "Matrix message TTL index missing");
  const account = await User.create({ fullName: "Synthetic", email: "native-registration@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const peer = await User.create({ fullName: "Synthetic peer", email: "native-peer@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const app = express(); app.use(express.json()); app.use(cookieParser()); app.use("/api/auth", authRoutes);
  // Isolated synthetic sessions use separate router instances so this long
  // integration scenario does not share a single process-local IP quota.
  const deviceRouters = new Map();
  let fixtureRequestCount = 0;
  app.use("/api/matrix/device", (req, res, next) => {
    const key = `${req.cookies.jwt || "anonymous"}:${req.path.split("/")[1] || "root"}:${fixtureRequestCount++ % 4}`;
    if (!deviceRouters.has(key)) deviceRouters.set(key, createMatrixDeviceRouter("sonar.invalid"));
    deviceRouters.get(key)(req, res, next);
  });
  let rejectFirstShare = true, rejectFirstEvent = true;
  app.use("/api/matrix/messages", (req, res, next) => { if (req.path === "/shares/send" && rejectFirstShare) { rejectFirstShare = false; return res.sendStatus(503); } if (req.path === "/events/send" && rejectFirstEvent) { rejectFirstEvent = false; return res.sendStatus(503); } next(); }, createMatrixMessagingRouter("sonar.invalid"));
  const roomId = "!native-synthetic:sonar.invalid", userId = `@${account._id}:sonar.invalid`;
  app.post("/synthetic/setup", async (_req, res) => {
    try {
      const session = await LoginSession.findOne({ userId: account._id, "matrixDevice.deviceId": "NATIVE" }).lean();
      await createMatrixRoom(roomId, userId, [userId, "@000000000000000000000000:sonar.invalid"], "direct");
      await bootstrapMatrixDevice(String(account._id), session._id, "sonar.invalid", "NATIVE");
      res.sendStatus(204);
    } catch { res.sendStatus(500); }
  });
  app.post("/synthetic/authorize-third", async (_req, res) => {
    const updated = await MatrixDevice.updateOne({ userId, deviceId: "THIRD", serverAuthorization: "pending" }, { $set: { serverAuthorization: "authorized" } });
    res.sendStatus(updated.modifiedCount === 1 ? 204 : 400);
  });
  app.post("/synthetic/reset-third", async (_req, res) => { await MatrixDevice.updateOne({ userId, deviceId: "THIRD" }, { $set: { serverAuthorization: "pending" } }); res.sendStatus(204); });
  app.post("/synthetic/bootstrap-peer", async (_req, res) => {
    try { const peerSession = await LoginSession.findOne({ userId: peer._id, "matrixDevice.deviceId": "PEER" }).lean(); await bootstrapMatrixDevice(String(peer._id), peerSession._id, "sonar.invalid", "PEER"); res.sendStatus(204); } catch { res.sendStatus(400); }
  });
  app.get("/synthetic/contact-inbox", async (req, res) => { try { const peerSession = await LoginSession.findOne({ userId: peer._id, "matrixDevice.deviceId": "PEER" }).lean(); const rows = await readSessionContactVerification(String(peer._id), peerSession._id, "sonar.invalid", "PEER", req.query.roomId); res.json({ count: rows.length }); } catch { res.sendStatus(400); } });
  app.post("/synthetic/advance", async (_req, res) => { await MatrixRoom.updateOne({ roomId }, { $inc: { revision: 1 } }); res.sendStatus(204); });
  app.post("/synthetic/advance-direct", async (req, res) => { const result = await MatrixRoom.updateOne({ roomId: req.body.roomId, kind: "direct", members: { $all: [userId, `@${peer._id}:sonar.invalid`] } }, { $inc: { revision: 1 } }); res.sendStatus(result.modifiedCount === 1 ? 204 : 400); });
  app.post("/synthetic/expire-event", async (req, res) => { if (!/^[a-f0-9]{24}$/.test(req.body.eventId || "")) return res.sendStatus(400); const result = await MatrixRoomEvent.updateOne({ _id: req.body.eventId, sender: userId, pinned: false }, { $set: { expiresAt: new Date(0) } }); res.sendStatus(result.modifiedCount === 1 ? 204 : 400); });
  app.post("/synthetic/rollback", async (_req, res) => { await MatrixRoom.updateOne({ roomId }, { $inc: { revision: -1 } }); res.sendStatus(204); });
  await new Promise(resolve => { server = app.listen(0, "127.0.0.1", resolve); });
  const executable = resolve(root, "desktop/target/debug/examples/matrix_register_probe.exe");
  child = spawn(executable, [], { windowsHide: true, env: { ...process.env, SONAR_MATRIX_TEST_BASE: `http://127.0.0.1:${server.address().port}`, SONAR_MATRIX_TEST_ROOT: temp, SONAR_MATRIX_TEST_PEER: String(peer._id) }, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "", stderr = ""; child.stdout.on("data", value => { stdout += value; }); child.stderr.on("data", value => { stderr += value; });
  const timer = setTimeout(() => child.kill(), 90000);
  const [code] = await once(child, "exit"); clearTimeout(timer);
  assert.equal(code, 0, stderr.slice(-1000));
  assert.match(stdout, /SONAR_MATRIX_REGISTER_OK/);
  const devices = await MatrixDevice.find({ userId: `@${account._id}:sonar.invalid`, revokedAt: null }).lean();
  assert.equal(devices.length, 2);
  assert.deepEqual(devices.map(device => device.serverAuthorization).sort(), ["authorized", "pending"]);
  console.log("PASS: native Matrix registration, verified messages, retention, device revocation and room-key rotation over isolated authenticated HTTP.");
} finally {
  if (child && child.exitCode === null && child.signalCode === null) { child.kill(); await once(child, "exit"); }
  if (server) await new Promise(resolve => server.close(resolve));
  await mongoose.disconnect(); await db.stop();
  if (resolve(temp).startsWith(resolve(tmpdir()) + sep) && dirname(resolve(temp)) === resolve(tmpdir())) await rm(temp, { recursive: true, force: true });
}
