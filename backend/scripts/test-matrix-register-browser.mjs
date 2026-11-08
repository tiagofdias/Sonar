// Isolated browser + in-memory database; never touches an operator database.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import express from "express";
import cookieParser from "cookie-parser";
import jwt from "jsonwebtoken";
import mongoose from "mongoose";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { chromium } from "playwright";
import User from "../src/models/user.model.js";
import LoginSession from "../src/models/loginSession.model.js";
import MatrixDevice from "../src/models/matrixDevice.model.js";
import MatrixAccount from "../src/models/matrixAccount.model.js";
import MatrixEnrollment from "../src/models/matrixEnrollment.model.js";
import MatrixBindingChallenge from "../src/models/matrixBindingChallenge.model.js";
import MatrixBootstrapChallenge from "../src/models/matrixBootstrapChallenge.model.js";
import { createSession } from "../src/lib/sessions.js";
import { createMatrixDeviceRouter } from "../src/routes/matrixDevice.route.js";
import { createMatrixMessagingRouter } from "../src/routes/matrixMessaging.route.js";
import { protectRoute } from "../src/middleware/auth.middleware.js";
import { createMatrixRoom } from "../src/lib/matrixRooms.js";
import MatrixRoom from "../src/models/matrixRoom.model.js";
import MatrixRoomEvent from "../src/models/matrixRoomEvent.model.js";

process.env.JWT_SECRET = "synthetic-browser-matrix-registration-secret";
process.env.VITE_SONAR_MATRIX_EXPERIMENTAL = "true";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const requireFrontend = createRequire(resolve(root, "frontend/package.json"));
const { createServer } = await import(pathToFileURL(requireFrontend.resolve("vite")).href);
const db = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
let server, vite, browser;
try {
  await mongoose.connect(db.getUri("sonar-browser-matrix-registration"));
  await Promise.all([User.init(), LoginSession.init(), MatrixDevice.init(), MatrixAccount.init(), MatrixEnrollment.init(), MatrixBindingChallenge.init(), MatrixBootstrapChallenge.init()]);
  const account = await User.create({ fullName: "Synthetic", email: "matrix-browser@example.invalid", password: "unused-test-hash" });
  const accountId = String(account._id), userId = `@${accountId}:sonar.invalid`;
  const sessionId = await createSession(accountId, 0, "password");
  const token = jwt.sign({ userId: accountId, sessionVersion: 0, sessionId }, process.env.JWT_SECRET);
  const app = express(); app.use(express.json()); app.use(cookieParser());
  const deviceRouters = new Map(); let deviceRequestCount = 0;
  app.use("/api/matrix/device", (req, res, next) => {
    const group = `${req.get("X-Sonar-Test-Contact") || ""}:${req.get("X-Sonar-Test-Fresh") || ""}:${req.get("X-Sonar-Test-Setup") || ""}:${deviceRequestCount++ % 4}`;
    if (!deviceRouters.has(group)) deviceRouters.set(group, createMatrixDeviceRouter("sonar.invalid"));
    deviceRouters.get(group)(req, res, next);
  });
  // This long synthetic scenario exercises many independent sessions in one
  // process; keep its request quota independent of production rate limits.
  const messageRouters = Array.from({ length: 4 }, () => createMatrixMessagingRouter("sonar.invalid"));
  let messageRequestCount = 0;
  app.use("/api/matrix/messages", (req, res, next) => messageRouters[messageRequestCount++ % messageRouters.length](req, res, next));
  app.get("/api/auth/check", protectRoute, (req, res) => res.json({ _id: req.user._id, fullName: req.user.fullName, email: req.user.email, encryptionKey: req.user.encryptionKey }));
  // Test-only fault injection; never mounted by the application.
  app.post("/test/advance-room", protectRoute, async (req, res) => {
    const result = await MatrixRoom.updateOne({ roomId: req.body.roomId, members: `@${req.user._id}:sonar.invalid` }, { $inc: { revision: 1 } });
    res.json({ updated: result.modifiedCount === 1 });
  });
  vite = await createServer({ root: resolve(root, "frontend"), configFile: resolve(root, "frontend/vite.config.js"), server: { middlewareMode: true, proxy: {} }, appType: "spa" });
  app.use(vite.middlewares);
  await new Promise(resolve => { server = app.listen(0, "127.0.0.1", resolve); });
  const base = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ headless: true, ...(process.env.SONAR_BROWSER_EXECUTABLE ? { executablePath: process.env.SONAR_BROWSER_EXECUTABLE } : {}) });
  const context = await browser.newContext();
  await context.addCookies([{ name: "jwt", value: token, url: base, httpOnly: true, sameSite: "Strict" }]);
  const page = await context.newPage(); await page.goto(base);
  const outcome = await page.evaluate(async user => {
    const { createBrowserMatrixIdentity, withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { registerExistingBrowserMatrixDevice, listBrowserMatrixDevices } = await import("/src/lib/matrixDeviceClient.js");
    const descriptor = await createBrowserMatrixIdentity(user, "BROWSER");
    let lost = false;
    const api = {
      async get(path) { const response = await fetch(`/api${path}`); if (!response.ok) throw new Error("HTTP error"); return { data: await response.json() }; },
      async post(path, body) {
        const response = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
        if (!response.ok) throw new Error("HTTP error");
        const data = await response.json();
        if (path.endsWith("/enroll/complete") && !lost) { lost = true; throw new Error("lost response"); }
        return { data };
      },
    };
    const registered = await registerExistingBrowserMatrixDevice(user, api);
    const retried = await registerExistingBrowserMatrixDevice(user, api);
    const devices = await listBrowserMatrixDevices(user, api);
    let substitutedRejected = false;
    const altered = { ...api, get: async path => { const response = await api.get(path); if (path.endsWith("/list")) response.data.devices[0].ed25519 = "A".repeat(43); return response; } };
    try { await listBrowserMatrixDevices(user, altered); } catch { substitutedRejected = true; }
    let changedRejected = false;
    try { await registerExistingBrowserMatrixDevice(user, { ...api, get: async () => ({ data: { userId: "@000000000000000000000000:other.invalid" } }) }); } catch { changedRejected = true; }
    const reopened = await withBrowserMatrixIdentity(user, (_machine, value) => value);
    return { descriptor, registered, retried, reopened, lost, changedRejected, devices, substitutedRejected };
  }, userId);
  assert.equal(outcome.lost, true);
  assert.equal(outcome.changedRejected, true);
  assert.equal(outcome.substitutedRejected, true);
  assert.equal(outcome.devices.length, 1);
  assert.equal(outcome.devices[0].serverAuthorization, "pending");
  assert.deepEqual(outcome.registered, { userId, deviceId: "BROWSER", status: "verification-required" });
  assert.deepEqual(outcome.retried, outcome.registered);
  assert.deepEqual(outcome.reopened, outcome.descriptor);
  assert.equal(await MatrixDevice.countDocuments({ userId }), 1);
  assert.equal((await MatrixDevice.findOne({ userId })).serverAuthorization, "pending");
  const roomId = "!browser-http:sonar.invalid";
  await createMatrixRoom(roomId, userId, [userId, "@000000000000000000000000:sonar.invalid"], "direct");
  const legacyPublicKey = await page.evaluate(async id => (await (await import("/src/lib/crypto.js")).identity(id)).publicKeyText, accountId);
  await User.updateOne({ _id: accountId }, { $set: { encryptionKey: legacyPublicKey } });
  const challengeAndProof = await page.evaluate(async ({ accountId, userId }) => {
    const response = await fetch("/api/matrix/device/bootstrap/legacy/challenge", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ deviceId: "BROWSER" }) });
    if (!response.ok) throw new Error("Challenge failed");
    const challenge = await response.json();
    const proof = await (await import("/src/lib/crypto.js")).proveExistingLegacyIdentity(accountId, userId, "BROWSER", challenge);
    return { challenge, proof };
  }, { accountId, userId });
  const otherContext = await browser.newContext();
  const otherPage = await otherContext.newPage(); await otherPage.goto(base);
  const missingKey = await otherPage.evaluate(async ({ accountId, userId, challenge }) => {
    let rejected = false;
    try { await (await import("/src/lib/crypto.js")).proveExistingLegacyIdentity(accountId, userId, "BROWSER", challenge); } catch { rejected = true; }
    const stored = await new Promise((resolve, reject) => {
      const request = indexedDB.open("sonar-keys", 1);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => { const get = request.result.transaction("keys").objectStore("keys").get(accountId); get.onsuccess = () => resolve(Boolean(get.result)); get.onerror = () => reject(get.error); };
    });
    return { rejected, stored };
  }, { accountId, userId, challenge: challengeAndProof.challenge });
  assert.deepEqual(missingKey, { rejected: true, stored: false });
  await otherContext.close();
  await User.updateOne({ _id: accountId }, { $set: { encryptionKey: "changed-for-test" } });
  const changedKeyRejected = await page.evaluate(async ({ challenge, proof }) => {
    const response = await fetch("/api/matrix/device/bootstrap/legacy/complete", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ deviceId: "BROWSER", token: challenge.token, proof }) });
    return !response.ok;
  }, challengeAndProof);
  assert.equal(changedKeyRejected, true);
  assert.equal((await MatrixDevice.findOne({ userId })).serverAuthorization, "pending");
  await User.updateOne({ _id: accountId }, { $set: { encryptionKey: legacyPublicKey } });
  const bootstrap = await page.evaluate(async user => {
    const { bootstrapExistingBrowserMatrixIdentity } = await import("/src/lib/matrixDeviceClient.js");
    const api = {
      async get(path) { const r = await fetch(`/api${path}`); if (!r.ok) throw new Error("HTTP error"); return { data: await r.json() }; },
      async post(path, body) { const r = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }); if (!r.ok) throw new Error("HTTP error"); return { data: await r.json() }; },
    };
    const challenge = (await api.post("/matrix/device/bootstrap/legacy/challenge", { deviceId: "BROWSER" })).data;
    let invalidRejected = false;
    try { await api.post("/matrix/device/bootstrap/legacy/complete", { deviceId: "BROWSER", token: challenge.token, proof: "A".repeat(43) }); } catch { invalidRejected = true; }
    let lost = false;
    const flaky = { ...api, async post(path, body) { const result = await api.post(path, body); if (path.endsWith("/bootstrap/legacy/complete") && !lost) { lost = true; throw new Error("lost response"); } return result; } };
    const result = await bootstrapExistingBrowserMatrixIdentity(user, flaky);
    const retried = await bootstrapExistingBrowserMatrixIdentity(user, api);
    let substitutedRejected = false;
    const altered = { ...api, async get(path) { const response = await api.get(path); if (path.endsWith("/list")) response.data.devices[0].ed25519 = "A".repeat(43); return response; } };
    try { await bootstrapExistingBrowserMatrixIdentity(user, altered); } catch { substitutedRejected = true; }
    return { invalidRejected, lost, retried, substitutedRejected, result };
  }, userId);
  assert.equal(bootstrap.invalidRejected, true);
  assert.equal(bootstrap.lost, true);
  assert.equal(bootstrap.substitutedRejected, true);
  assert.deepEqual(bootstrap.retried, bootstrap.result);
  assert.deepEqual(bootstrap.result, { userId, deviceId: "BROWSER", status: "authorized" });
  assert.equal((await User.findById(accountId)).encryptionKey, legacyPublicKey);
  assert.equal((await MatrixAccount.findById(userId)).bootstrapped, true);
  const sync = async ({ userId, roomId }) => {
    const { syncBrowserMatrixRoom } = await import("/src/lib/matrixDeviceClient.js");
    const api = { async get(path, options) {
      const query = options?.params ? `?${new URLSearchParams(options.params)}` : "";
      const response = await fetch(`/api${path}${query}`);
      if (!response.ok) throw new Error("HTTP error");
      return { data: await response.json() };
    } };
    return syncBrowserMatrixRoom(userId, roomId, api);
  };
  const initial = await page.evaluate(sync, { userId, roomId });
  assert.equal(initial.revision, 1); assert.equal(initial.rotated, true);
  const repeat = await page.evaluate(sync, { userId, roomId });
  assert.equal(repeat.rotated, false);
  await MatrixRoom.updateOne({ roomId }, { $inc: { revision: 1 } });
  const changed = await page.evaluate(sync, { userId, roomId });
  assert.equal(changed.revision, 2); assert.equal(changed.rotated, true);
  await MatrixRoom.updateOne({ roomId }, { $inc: { revision: -1 } });
  await assert.rejects(page.evaluate(sync, { userId, roomId }), /revision changed unexpectedly/);
  const second = await User.create({ fullName: "Synthetic second", email: "matrix-setup-second@example.invalid", password: "unused-test-hash" });
  const secondAccount = String(second._id), secondSession = await createSession(secondAccount, 0, "password");
  const secondToken = jwt.sign({ userId: secondAccount, sessionVersion: 0, sessionId: secondSession }, process.env.JWT_SECRET);
  const setupContext = await browser.newContext({ extraHTTPHeaders: { "X-Sonar-Test-Setup": "1" } });
  await setupContext.addCookies([{ name: "jwt", value: secondToken, url: base, httpOnly: true, sameSite: "Strict" }]);
  const setupPage = await setupContext.newPage(); await setupPage.goto(base);
  const setup = await setupPage.evaluate(async account => {
    const { prepareBrowserMatrixDevice } = await import("/src/lib/matrixDeviceClient.js");
    const api = {
      async get(path) { const r = await fetch(`/api${path}`); if (!r.ok) throw new Error("HTTP error"); return { data: await r.json() }; },
      async post(path, body) { const r = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }); if (!r.ok) throw new Error("HTTP error"); return { data: await r.json() }; },
    };
    return Promise.all([prepareBrowserMatrixDevice(account, api), prepareBrowserMatrixDevice(account, api)]);
  }, secondAccount);
  assert.deepEqual(setup[0], setup[1]);
  assert.equal(setup[0].status, "verification-required");
  assert.equal(await MatrixDevice.countDocuments({ userId: `@${secondAccount}:sonar.invalid` }), 1);
  const secondLegacy = await setupPage.evaluate(async id => (await (await import("/src/lib/crypto.js")).identity(id)).publicKeyText, secondAccount);
  await User.updateOne({ _id: secondAccount }, { $set: { encryptionKey: secondLegacy } });
  await setupPage.evaluate(async accountId => {
    (await import("/src/lib/matrixDeviceSettings.browser-fixture.jsx")).mountDeviceSettings(accountId);
  }, secondAccount);
  await setupPage.getByRole("button", { name: "Prepare this browser" }).click();
  await setupPage.getByRole("button", { name: "Authorize this first device" }).click();
  await setupPage.getByRole("status").filter({ hasText: "authorized on the server" }).waitFor();
  assert.equal((await MatrixDevice.findOne({ userId: `@${secondAccount}:sonar.invalid` })).serverAuthorization, "authorized");
  assert.equal((await User.findById(secondAccount)).encryptionKey, secondLegacy);
  const prepareRoom = async ({ own, peer }) => {
    const { prepareBrowserDirectMatrixRoom } = await import("/src/lib/matrixDeviceClient.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    return prepareBrowserDirectMatrixRoom(`@${own}:sonar.invalid`, peer, api);
  };
  const firstRoom = await page.evaluate(prepareRoom, { own: accountId, peer: secondAccount });
  const secondRoom = await setupPage.evaluate(prepareRoom, { own: secondAccount, peer: accountId });
  assert.equal(firstRoom.roomId, secondRoom.roomId);
  assert.deepEqual(firstRoom.members, secondRoom.members);
  const senderDevice = await MatrixDevice.findOne({ userId, deviceId: "BROWSER" }).lean();
  const peerMatrixUser = `@${secondAccount}:sonar.invalid`;
  const receiverDevice = await MatrixDevice.findOne({ userId: peerMatrixUser, deviceId: setup[0].deviceId }).lean();
  const scopedVerification = await page.evaluate(async ({ room, peerUser, peerDevice, senderKey, recipientKey }) => {
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    const messages = { [peerUser]: { [peerDevice]: { algorithm: "m.olm.v1.curve25519-aes-sha2", sender_key: senderKey, ciphertext: { [recipientKey]: { type: 1, body: "AQ" } } } } };
    const payload = { deviceId: "BROWSER", roomId: room.roomId, revision: room.revision, transactionId: "synthetic-contact-verification", eventType: "m.room.encrypted", messages };
    const result = (await api.post("/matrix/messages/verification/send", payload)).data;
    let wrongRevision = false;
    try { await api.post("/matrix/messages/verification/send", { ...payload, revision: room.revision + 1, transactionId: "wrong-room-revision" }); } catch { wrongRevision = true; }
    return { result, wrongRevision };
  }, { room: firstRoom, peerUser: peerMatrixUser, peerDevice: setup[0].deviceId, senderKey: senderDevice.deviceKeys.keys["curve25519:BROWSER"], recipientKey: receiverDevice.deviceKeys.keys[`curve25519:${setup[0].deviceId}`] });
  assert.deepEqual(scopedVerification, { result: { accepted: true }, wrongRevision: true });
  const scopedInbox = await setupPage.evaluate(async ({ roomId, deviceId }) => {
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    const events = (await api.get("/matrix/messages/verification/inbox", { params: { roomId, deviceId } })).data.events;
    const shares = (await api.get("/matrix/messages/shares/inbox", { params: { roomId, deviceId } })).data.events;
    await api.post("/matrix/messages/verification/ack", { roomId, deviceId, ids: events.map(event => event.id) });
    return { count: events.length, sender: events[0]?.event?.sender, shares: shares.length };
  }, { roomId: firstRoom.roomId, deviceId: setup[0].deviceId });
  assert.deepEqual(scopedInbox, { count: 1, sender: userId, shares: 0 });
  const trustCheck = await page.evaluate(async ({ userId, room }) => {
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { requireTrustedRoomDevices } = await import("/src/lib/matrixRoomKeys.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    try { await withBrowserMatrixIdentity(userId, (machine, descriptor) => requireTrustedRoomDevices(machine, descriptor, room, api)); return "unexpected success"; }
    catch (error) { return error.message; }
  }, { userId, room: firstRoom });
  assert.match(trustCheck, /not verified/i);
  await setupPage.evaluate(async user => {
    const sdk = await import("/src/lib/matrixSdk.browser-fixture.js");
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { publishMatrixVerificationKeys } = await import("/src/lib/matrixVerificationKeys.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    await withBrowserMatrixIdentity(user, (machine, descriptor) => publishMatrixVerificationKeys(sdk, machine, descriptor, api));
  }, `@${secondAccount}:sonar.invalid`);
  const established = await page.evaluate(async ({ userId, peerUserId, peerDeviceId, room }) => {
    const sdk = await import("/src/lib/matrixSdk.browser-fixture.js");
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { establishTrustedRoomSessions } = await import("/src/lib/matrixRoomKeys.js");
    const { prepareTrustedRoomKeyShares, deliverTrustedRoomKeyShares, discardStaleRoomKeyShares } = await import("/src/lib/matrixShareOutbox.js");
    const { queueEncryptedRoomEvent, deliverEncryptedRoomEvent, pendingEncryptedRoomEvents } = await import("/src/lib/matrixEventOutbox.js");
    const { sendTrustedRoomText, PendingMatrixMessage } = await import("/src/lib/matrixRoomSend.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    return withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
      const user = new sdk.UserId(peerUserId), id = new sdk.DeviceId(peerDeviceId);
      const peer = await machine.getDevice(user, id);
      try {
        // Test-only local trust; real clients must complete independent SAS.
        await peer.setLocalTrust(sdk.LocalTrust.Verified);
        const result = await establishTrustedRoomSessions(machine, descriptor, room, api);
        let interrupted = false;
        const interruptedMachine = new Proxy(machine, { get(target, property) {
          if (property === "shareRoomKey") return async (...args) => { await target.shareRoomKey(...args); interrupted = true; throw new Error("simulated interruption after SDK share"); };
          const value = Reflect.get(target, property, target); return typeof value === "function" ? value.bind(target) : value;
        } });
        try { await prepareTrustedRoomKeyShares(interruptedMachine, descriptor, room, api); } catch (error) { if (!interrupted) throw error; }
        if (!interrupted) throw new Error("Interruption was not simulated");
        let blocked = false;
        try { await deliverTrustedRoomKeyShares(machine, descriptor, room, api); } catch { blocked = true; }
        if (!blocked) throw new Error("Interrupted key preparation was not blocked");
        const oldShares = await prepareTrustedRoomKeyShares(machine, descriptor, room, api);
        if (oldShares !== 1) throw new Error("Expected a pending room key");
        const advanced = await fetch("/test/advance-room", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ roomId: room.roomId }) });
        if (!(await advanced.json()).updated) throw new Error("Could not advance test room");
        let staleBlocked = false;
        try { await deliverTrustedRoomKeyShares(machine, descriptor, room, api); } catch { staleBlocked = true; }
        if (!staleBlocked) throw new Error("Stale share was not blocked");
        room = { ...room, revision: room.revision + 1 };
        if (await discardStaleRoomKeyShares(machine, descriptor, room, api) !== 1) throw new Error("Stale share was not discarded");
        const shares = await prepareTrustedRoomKeyShares(machine, descriptor, room, api);
        let shareLost = false;
        const flakyShare = { get: api.get.bind(api), async post(path, body) { const response = await api.post(path, body); if (path.endsWith("/shares/send") && !shareLost) { shareLost = true; throw new Error("lost share response"); } return response; } };
        try { await deliverTrustedRoomKeyShares(machine, descriptor, room, flakyShare); } catch (error) { if (!shareLost) throw error; }
        if (!shareLost || await deliverTrustedRoomKeyShares(machine, descriptor, room, api) !== shares) throw new Error("Room key retry failed");
        const content = JSON.parse(await machine.encryptRoomEvent(new sdk.RoomId(room.roomId), "m.room.message", JSON.stringify({ msgtype: "m.text", body: "synthetic private message" })));
        const outboxId = await queueEncryptedRoomEvent(machine, descriptor, room, content, api);
        let lost = false;
        const flaky = { get: api.get.bind(api), async post(path, body) { const response = await api.post(path, body); if (path.endsWith("/events/send") && !lost) { lost = true; throw new Error("lost response"); } return response; } };
        try { await deliverEncryptedRoomEvent(machine, descriptor, room, outboxId, flaky); throw new Error("Expected lost response"); } catch (error) { if (!lost) throw error; }
        if ((await pendingEncryptedRoomEvents(descriptor)).length !== 1) throw new Error("Outbox lost its pending message");
        const sent = await deliverEncryptedRoomEvent(machine, descriptor, room, outboxId, api);
        if (!sent.duplicate || (await pendingEncryptedRoomEvents(descriptor)).length !== 0) throw new Error("Outbox retry failed");
        let secondLost = false, pendingId;
        const secondFlaky = { get: api.get.bind(api), async post(path, body) { const response = await api.post(path, body); if (path.endsWith("/events/send") && !secondLost) { secondLost = true; throw new Error("lost composed message response"); } return response; } };
        try { await sendTrustedRoomText(machine, descriptor, room, "synthetic second message", secondFlaky); }
        catch (error) { if (!(error instanceof PendingMatrixMessage)) throw error; pendingId = error.outboxId; }
        if (!secondLost || !pendingId) throw new Error("Composed message was not retained");
        let secondBlocked = false;
        try { await sendTrustedRoomText(machine, descriptor, room, "must wait", api); } catch { secondBlocked = true; }
        if (!secondBlocked || !(await deliverEncryptedRoomEvent(machine, descriptor, room, pendingId, api)).duplicate) throw new Error("Composed message retry failed");
        return { claimed: result.claimed, devices: result.trusted.length, shares };
      } finally { await peer.setLocalTrust(sdk.LocalTrust.Unset); peer.free(); user.free(); id.free(); }
    });
  }, { userId, peerUserId: `@${secondAccount}:sonar.invalid`, peerDeviceId: setup[0].deviceId, room: firstRoom });
  assert.deepEqual(established, { claimed: 1, devices: 2, shares: 1 });
  firstRoom.revision++; secondRoom.revision++;
  const encryptedRows = await MatrixRoomEvent.find({ roomId: firstRoom.roomId }).lean();
  assert.equal(encryptedRows.length, 2);
  assert.equal(JSON.stringify(encryptedRows).includes("synthetic private message"), false);
  const receivedKeys = await setupPage.evaluate(async ({ own, peerUser, peerDevice, room }) => {
    const sdk = await import("/src/lib/matrixSdk.browser-fixture.js");
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { requireTrustedRoomDevices } = await import("/src/lib/matrixRoomKeys.js");
    const { receiveTrustedRoomKeys } = await import("/src/lib/matrixRoomInbox.js");
    const { readTrustedRoomHistory } = await import("/src/lib/matrixRoomHistory.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    return withBrowserMatrixIdentity(own, async (machine, descriptor) => {
      try { await requireTrustedRoomDevices(machine, descriptor, room, api); } catch { /* populate SDK keys, still untrusted */ }
      const user = new sdk.UserId(peerUser), id = new sdk.DeviceId(peerDevice), peer = await machine.getDevice(user, id);
      try {
        await peer.setLocalTrust(sdk.LocalTrust.Verified); // synthetic test only
        let lost = false;
        const flaky = { get: api.get.bind(api), async post(path, body) { if (path.endsWith("/shares/ack") && !lost) { lost = true; throw new Error("lost acknowledgement"); } return api.post(path, body); } };
        let firstFailed = false;
        try { await receiveTrustedRoomKeys(machine, descriptor, room, flaky); } catch (error) { if (!lost) throw error; firstFailed = true; }
        const received = await receiveTrustedRoomKeys(machine, descriptor, room, api);
        const history = await readTrustedRoomHistory(machine, descriptor, room, api);
        return { firstFailed, received, remaining: (await api.get("/matrix/messages/shares/inbox", { params: { deviceId: descriptor.deviceId, roomId: room.roomId } })).data.events.length, texts: history.messages.map(message => message.text) };
      } finally { await peer.setLocalTrust(sdk.LocalTrust.Unset); peer.free(); user.free(); id.free(); }
    });
  }, { own: `@${secondAccount}:sonar.invalid`, peerUser: userId, peerDevice: "BROWSER", room: secondRoom });
  assert.deepEqual(receivedKeys, { firstFailed: true, received: 1, remaining: 0, texts: ["synthetic private message", "synthetic second message"] });
  const openContact = async (browserPage, own, targetUser, targetDevice, room, loseFirst = false, loseAck = false) => {
    await browserPage.evaluate(async args => {
      const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
      const { prepareContactVerification } = await import("/src/lib/matrixContactVerification.js");
      const { axiosInstance } = await import("/src/lib/axios.js");
      let lose = args.loseFirst, ackBefore = args.loseAck, ackAfter = args.loseAck;
      const api = { get: axiosInstance.get.bind(axiosInstance), async post(path, body) { if (ackBefore && path.endsWith("/verification/ack")) { ackBefore = false; throw new Error("ack lost before server"); } const result = await axiosInstance.post(path, body); if (lose && path.endsWith("/verification/send")) { lose = false; throw new Error("lost contact verification response"); } if (ackAfter && path.endsWith("/verification/ack")) { ackAfter = false; throw new Error("ack lost after server"); } return result; } };
      window.contactError = null;
      window.contactPromise = withBrowserMatrixIdentity(args.own, async (machine, descriptor) => {
        let flow = await prepareContactVerification(machine, descriptor, args.room, args.targetUser, args.targetDevice, api);
        window.newContactFlow = async () => { flow.close(); flow = await prepareContactVerification(machine, descriptor, args.room, args.targetUser, args.targetDevice, api); window.contactFlow = flow; };
        window.contactFlow = flow;
        try { await new Promise(resolve => { window.endContact = resolve; }); } finally { flow.close(); }
      }).catch(error => { window.contactError = error.message; });
    }, { own, targetUser, targetDevice, room, loseFirst, loseAck });
    await browserPage.waitForFunction(() => window.contactFlow || window.contactError);
    const error = await browserPage.evaluate(() => window.contactError);
    if (error) throw new Error(error);
  };
  await openContact(page, userId, peerMatrixUser, setup[0].deviceId, firstRoom, true);
  await openContact(setupPage, peerMatrixUser, userId, "BROWSER", secondRoom, false, true);
  await assert.rejects(page.evaluate(() => window.contactFlow.start()), /lost contact verification response/);
  assert.equal(await page.evaluate(() => window.contactFlow.hasPendingDelivery), true);
  const contactFlowId = await page.evaluate(async () => { const id = window.contactFlow.id; await window.contactFlow.retryDelivery(); return id; });
  await assert.rejects(setupPage.evaluate(() => window.contactFlow.receive()), /ack lost before server/);
  await assert.rejects(setupPage.evaluate(() => window.contactFlow.receive()), /ack lost after server/);
  const incomingContact = await setupPage.evaluate(async () => { await window.contactFlow.receive(); return window.contactFlow.incoming(); });
  assert.deepEqual(incomingContact, [{ flowId: contactFlowId, userId, deviceId: "BROWSER" }]);
  await page.evaluate(() => window.contactFlow.cancel());
  assert.deepEqual(await setupPage.evaluate(async () => { await window.contactFlow.receive(); return window.contactFlow.incoming(); }), []);
  await page.evaluate(() => window.newContactFlow());
  await setupPage.evaluate(() => window.newContactFlow());
  const newContactFlowId = await page.evaluate(() => window.contactFlow.start());
  await setupPage.evaluate(async id => { await window.contactFlow.receive(); await window.contactFlow.accept(id); }, newContactFlowId);
  await page.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.startSas(); });
  await setupPage.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.acceptSas(); });
  await page.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.flush(); });
  await setupPage.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.flush(); });
  await page.evaluate(() => window.contactFlow.receive());
  assert.deepEqual(await page.evaluate(() => window.contactFlow.codes()), await setupPage.evaluate(() => window.contactFlow.codes()));
  assert.equal(await page.evaluate(() => window.contactFlow.verified()), false);
  await page.evaluate(() => window.contactFlow.confirm(true));
  await setupPage.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.confirm(true); });
  await page.evaluate(async () => { await window.contactFlow.receive(); await window.contactFlow.flush(); });
  await setupPage.evaluate(async () => { await window.contactFlow.flush(); await window.contactFlow.receive(); });
  await page.evaluate(() => window.contactFlow.receive());
  assert.equal(await page.evaluate(() => window.contactFlow.verified()), true);
  assert.equal(await setupPage.evaluate(() => window.contactFlow.verified()), true);
  await page.evaluate(async () => { window.endContact(); await window.contactPromise; });
  await setupPage.evaluate(async () => { window.endContact(); await window.contactPromise; });
  const afterContactVerification = await page.evaluate(async ({ userId, room }) => {
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { requireTrustedRoomDevices } = await import("/src/lib/matrixRoomKeys.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    return withBrowserMatrixIdentity(userId, async (machine, descriptor) => (await requireTrustedRoomDevices(machine, descriptor, room, api)).length);
  }, { userId, room: firstRoom });
  assert.equal(afterContactVerification, 2);
  await page.setExtraHTTPHeaders({ "X-Sonar-Test-Contact": "a" });
  await setupPage.setExtraHTTPHeaders({ "X-Sonar-Test-Contact": "b" });
  await page.evaluate(async ({ own, peer }) => (await import("/src/lib/matrixContact.browser-fixture.jsx")).mountContactVerification(own, peer, "Synthetic peer"), { own: accountId, peer: secondAccount });
  await setupPage.evaluate(async ({ own, peer }) => (await import("/src/lib/matrixContact.browser-fixture.jsx")).mountContactVerification(own, peer, "Synthetic contact"), { own: secondAccount, peer: accountId });
  const contactClick = async (browserPage, name) => {
    await browserPage.getByRole("button", { name, exact: true }).click();
    await browserPage.waitForFunction(() => document.querySelector("[data-contact-verification]")?.getAttribute("aria-busy") === "false");
  };
  await page.getByLabel("Contact device ID").fill(setup[0].deviceId);
  await setupPage.getByLabel("Contact device ID").fill("BROWSER");
  await contactClick(page, "Open contact verification");
  await contactClick(setupPage, "Open contact verification");
  await contactClick(page, "Request contact verification");
  await contactClick(setupPage, "Check contact requests");
  await contactClick(setupPage, "Accept contact request");
  await contactClick(page, "Receive contact updates"); await contactClick(page, "Start contact code exchange");
  await contactClick(setupPage, "Receive contact updates"); await contactClick(setupPage, "Accept contact code exchange");
  await contactClick(page, "Receive contact updates"); await contactClick(page, "Send contact events");
  await contactClick(setupPage, "Receive contact updates"); await contactClick(setupPage, "Send contact events");
  await contactClick(page, "Receive contact updates");
  await contactClick(page, "Show contact codes"); await contactClick(setupPage, "Show contact codes");
  assert.equal(await page.getByText("Contact codes:", { exact: false }).textContent(), await setupPage.getByText("Contact codes:", { exact: false }).textContent());
  await contactClick(page, "Check contact verification");
  await page.getByText("Waiting for both confirmations.").waitFor();
  await contactClick(page, "Contact codes match");
  await contactClick(setupPage, "Receive contact updates"); await contactClick(setupPage, "Contact codes match");
  await contactClick(page, "Receive contact updates"); await contactClick(page, "Send contact events");
  await contactClick(setupPage, "Send contact events"); await contactClick(setupPage, "Receive contact updates");
  await contactClick(page, "Receive contact updates"); await contactClick(page, "Check contact verification");
  await page.getByText("Contact verification completed.").waitFor();
  await contactClick(page, "Close contact verification"); await contactClick(setupPage, "Close contact verification");
  await page.evaluate(async ({ own, peer }) => (await import("/src/lib/matrixChat.browser-fixture.jsx")).mountMatrixChat(own, peer, "Synthetic peer"), { own: accountId, peer: secondAccount });
  await setupPage.evaluate(async ({ own, peer }) => (await import("/src/lib/matrixChat.browser-fixture.jsx")).mountMatrixChat(own, peer, "Synthetic contact"), { own: secondAccount, peer: accountId });
  await page.waitForFunction(() => document.querySelector("[data-matrix-chat]")?.getAttribute("aria-busy") === "false");
  await setupPage.waitForFunction(() => document.querySelector("[data-matrix-chat]")?.getAttribute("aria-busy") === "false");
  let loseUiMessage = true;
  await page.route("**/api/matrix/messages/events/send", async route => { if (!loseUiMessage) return route.continue(); loseUiMessage = false; await route.fetch(); await route.abort("failed"); });
  await page.getByLabel("Matrix message", { exact: true }).fill("sent through the experimental interface");
  await page.getByRole("button", { name: "Send Matrix message" }).click();
  await page.getByRole("button", { name: "Retry pending message" }).click();
  await page.getByText("Pending encrypted message delivered.").waitFor();
  assert.equal(loseUiMessage, false);
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await setupPage.getByText("sent through the experimental interface", { exact: false }).waitFor();
  await page.getByRole("button", { name: "Refresh Matrix history" }).click();
  const messageId = await page.getByRole("listitem").filter({ hasText: "sent through the experimental interface" }).first().getAttribute("data-matrix-message-id");
  const ownUiMessage = page.locator(`[data-matrix-message-id="${messageId}"]`);
  const otherUiMessage = setupPage.locator(`[data-matrix-message-id="${messageId}"]`);
  await otherUiMessage.getByRole("button", { name: "Reply to Matrix message" }).click();
  await setupPage.getByLabel("Matrix message", { exact: true }).fill("encrypted reply from recipient");
  await setupPage.getByRole("button", { name: "Send Matrix message" }).click();
  await setupPage.getByRole("listitem").filter({ hasText: "encrypted reply from recipient" }).waitFor({ timeout: 10000 }).catch(async error => { throw new Error(`Reply failed: ${await setupPage.locator('[data-matrix-chat] [role="alert"]').allTextContents()} (${error.message})`); });
  await page.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  const reply = page.getByRole("listitem").filter({ hasText: "encrypted reply from recipient" });
  await reply.waitFor();
  assert.match(await reply.textContent(), /Reply to: sent through the experimental interface/);
  const replyId = await reply.getAttribute("data-matrix-message-id");
  const storedReply = await MatrixRoomEvent.findById(replyId).lean();
  assert.equal(JSON.stringify(storedReply.content).includes(messageId), false);
  assert.equal(JSON.stringify(storedReply.content).includes("encrypted reply from recipient"), false);
  await ownUiMessage.getByRole("button", { name: "Add 👍 Matrix reaction" }).click();
  await ownUiMessage.getByRole("button", { name: "Remove 👍 Matrix reaction" }).waitFor();
  const storedReaction = await MatrixRoomEvent.findOne({ roomId: firstRoom.roomId, sender: userId }).sort({ createdAt: -1, _id: -1 }).lean();
  assert.equal(JSON.stringify(storedReaction.content).includes(messageId), false);
  assert.equal(JSON.stringify(storedReaction.content).includes("👍"), false);
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await setupPage.waitForFunction(() => [...document.querySelectorAll('[data-matrix-chat] button')].some(button => button.getAttribute('aria-label') === 'Add 👍 Matrix reaction' && button.textContent.includes('1')));
  await ownUiMessage.getByRole("button", { name: "Remove 👍 Matrix reaction" }).click();
  await ownUiMessage.getByRole("button", { name: "Add 👍 Matrix reaction" }).waitFor();
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await ownUiMessage.getByRole("button", { name: "Pin Matrix message" }).click();
  await ownUiMessage.getByText("(Pinned)", { exact: true }).waitFor();
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await otherUiMessage.getByText("(Pinned)", { exact: true }).waitFor();
  await setupPage.getByRole("button", { name: "Show pinned Matrix messages" }).click();
  await setupPage.getByRole("button", { name: "Show all Matrix messages" }).waitFor();
  assert.equal(await otherUiMessage.isVisible(), true);
  assert.equal(await setupPage.getByText("encrypted reply from recipient", { exact: false }).count(), 0);
  await setupPage.evaluate(() => { window.testRealNow = Date.now; Date.now = () => window.testRealNow() + 8 * 24 * 60 * 60 * 1000; document.dispatchEvent(new Event("visibilitychange")); });
  await setupPage.getByText("synthetic private message", { exact: false }).waitFor({ state: "detached" });
  assert.equal(await otherUiMessage.isVisible(), true);
  await setupPage.evaluate(() => { Date.now = window.testRealNow; });
  const interfaceRows = await MatrixRoomEvent.find({ roomId: firstRoom.roomId }).lean();
  assert.equal(interfaceRows.length, 4);
  assert.equal(JSON.stringify(interfaceRows).includes("fresh message after discard"), false);
  assert.equal(JSON.stringify(interfaceRows).includes("sent through the experimental interface"), false);
  assert.equal(JSON.stringify(interfaceRows).includes("encrypted reply from recipient"), false);
  await ownUiMessage.getByRole("button", { name: "Unpin Matrix message" }).click();
  await ownUiMessage.getByRole("button", { name: "Pin Matrix message" }).waitFor();
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await otherUiMessage.waitFor({ state: "detached" });
  await setupPage.getByRole("button", { name: "Show all Matrix messages" }).click();
  await otherUiMessage.getByRole("button", { name: "Pin Matrix message" }).waitFor();
  await ownUiMessage.getByRole("button", { name: "Delete Matrix message" }).click();
  await page.getByRole("button", { name: "Confirm Matrix deletion" }).click();
  await ownUiMessage.waitFor({ state: "detached" });
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  await otherUiMessage.waitFor({ state: "detached" });
  assert.equal(await MatrixRoomEvent.countDocuments({ roomId: firstRoom.roomId }), 3);
  let rejectDiscardedMessage = true;
  await page.route("**/api/matrix/messages/events/send", async route => { if (!rejectDiscardedMessage) return route.continue(); rejectDiscardedMessage = false; await route.abort("failed"); });
  await page.getByLabel("Matrix message", { exact: true }).fill("discarded before delivery");
  await page.getByRole("button", { name: "Send Matrix message" }).click();
  await page.getByRole("button", { name: "Discard pending retry" }).click();
  await page.getByText("Discard this local retry? The message may already have been delivered.").waitFor();
  await page.getByRole("button", { name: "Confirm discard" }).click();
  await page.getByText("Local retry discarded. The message may already have been delivered.").waitFor();
  await page.getByLabel("Matrix message", { exact: true }).fill("fresh message after discard");
  await page.getByRole("button", { name: "Send Matrix message" }).click();
  await page.getByRole("listitem").filter({ hasText: "fresh message after discard" }).waitFor();
  const syntheticFile = Buffer.from("synthetic private attachment bytes");
  let attachmentResponse = "no response";
  page.on("response", response => { if (response.url().includes("/events/send-attachment")) attachmentResponse = String(response.status()); });
  await page.getByLabel("Encrypted attachment (up to 10 MiB)").setInputFiles({ name: "synthetic-private.txt", mimeType: "text/plain", buffer: syntheticFile });
  await page.getByRole("button", { name: "Send encrypted attachment" }).click();
  await page.getByText("Encrypted attachment sent.").waitFor({ timeout: 10000 }).catch(async error => {
    if (await page.getByRole("button", { name: "Retry pending message" }).isVisible()) {
      await page.getByRole("button", { name: "Retry pending message" }).click();
      await page.waitForTimeout(1000);
    }
    throw new Error(`Attachment send failed (HTTP ${attachmentResponse}): ${await page.locator('[data-matrix-chat] [role="alert"]').allTextContents()} (${error.message})`);
  });
  await setupPage.evaluate(() => window.testMatrixSocketEvent("matrixRoomChanged"));
  const downloadButton = setupPage.getByRole("button", { name: /Download synthetic-private\.txt/ });
  await downloadButton.waitFor();
  const downloadPromise = setupPage.waitForEvent("download");
  await downloadButton.click();
  const download = await downloadPromise;
  const chunks = []; for await (const chunk of await download.createReadStream()) chunks.push(chunk);
  assert.deepEqual(Buffer.concat(chunks), syntheticFile);
  const attachmentRows = await MatrixRoomEvent.find({ roomId: firstRoom.roomId, attachmentId: { $exists: true } }).select("+attachmentCiphertext").lean();
  assert.equal(attachmentRows.length, 1);
  assert.equal(JSON.stringify(attachmentRows[0].content).includes("synthetic-private.txt"), false);
  assert.equal(Buffer.from(attachmentRows[0].attachmentCiphertext.buffer).includes(syntheticFile), false);

  await page.setExtraHTTPHeaders({});
  await setupPage.setExtraHTTPHeaders({ "X-Sonar-Test-Setup": "1" });
  const rejectedRoom = await setupPage.evaluate(async ({ own, peer }) => {
    const { prepareBrowserDirectMatrixRoom } = await import("/src/lib/matrixDeviceClient.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    const altered = { get: api.get.bind(api), async post(...args) { const result = await api.post(...args); result.data.members[1] = "@000000000000000000000000:sonar.invalid"; return result; } };
    try { await prepareBrowserDirectMatrixRoom(`@${own}:sonar.invalid`, peer, altered); return false; } catch { return true; }
  }, { own: secondAccount, peer: accountId });
  assert.equal(rejectedRoom, true);
  const freshContext = await browser.newContext({ extraHTTPHeaders: { "X-Sonar-Test-Fresh": "1" } });
  const freshSession = await createSession(secondAccount, 0, "password");
  const freshToken = jwt.sign({ userId: secondAccount, sessionVersion: 0, sessionId: freshSession }, process.env.JWT_SECRET);
  await freshContext.addCookies([{ name: "jwt", value: freshToken, url: base, httpOnly: true, sameSite: "Strict" }]);
  const freshPage = await freshContext.newPage(); await freshPage.goto(base);
  await freshPage.getByRole("heading", { name: "Set up this browser" }).waitFor();
  await freshPage.getByRole("button", { name: "Prepare this browser" }).click();
  await freshPage.getByRole("status").filter({ hasText: "awaiting authorization" }).waitFor();
  assert.equal(await freshPage.getByRole("button", { name: "Authorize this first device" }).count(), 0);
  const verification = await freshPage.evaluate(async account => {
    const { withBrowserMatrixVerification } = await import("/src/lib/matrixVerificationSession.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    const userId = `@${account}:sonar.invalid`;
    let closed;
    const result = await withBrowserMatrixVerification(userId, api, async session => {
      closed = session;
      return { deviceId: session.deviceId, serverAuthorization: session.serverAuthorization, requests: await session.receive() };
    });
    let closedRejected = false;
    try { await closed.receive(); } catch { closedRejected = true; }
    const altered = { ...api, get: async (...args) => { const response = await api.get(...args); if (args[0].endsWith("/list")) response.data.devices.find(device => device.current).ed25519 = "A".repeat(43); return response; }, post: api.post.bind(api) };
    let changedRejected = false;
    try { await withBrowserMatrixVerification(userId, altered, async () => {}); } catch { changedRejected = true; }
    return { ...result, closedRejected, changedRejected };
  }, secondAccount);
  assert.equal(verification.serverAuthorization, "pending");
  assert.deepEqual(verification.requests, []);
  assert.equal(verification.closedRejected, true);
  assert.equal(verification.changedRejected, true);
  const firstId = setup[0].deviceId, secondId = verification.deviceId;
  const click = async (page, name) => {
    await page.getByRole("button", { name, exact: true }).click();
    await page.waitForFunction(() => document.querySelector("[data-verification]")?.getAttribute("aria-busy") === "false");
  };
  await click(setupPage, "Open verification");
  await click(freshPage, "Open verification");
  await setupPage.getByLabel("Other device ID").fill(secondId);
  let loseVerificationResponse = true;
  await setupPage.route("**/api/matrix/device/verification/send", async route => {
    if (!loseVerificationResponse) return route.continue();
    loseVerificationResponse = false;
    await route.fetch();
    await route.abort("failed");
  });
  await click(setupPage, "Send verification request");
  await setupPage.getByRole("button", { name: "Retry verification delivery" }).waitFor();
  await click(setupPage, "Retry verification delivery");
  assert.equal(loseVerificationResponse, false);
  await click(freshPage, "Check for requests");
  await freshPage.getByText(`Request from ${firstId}`, { exact: false }).waitFor();
  await click(freshPage, "Accept request");
  await click(setupPage, "Receive updates"); await click(setupPage, "Start code exchange");
  await click(freshPage, "Receive updates"); await click(freshPage, "Accept code exchange");
  await click(setupPage, "Receive updates"); await click(setupPage, "Send pending events");
  await click(freshPage, "Receive updates"); await click(freshPage, "Send pending events");
  await click(setupPage, "Receive updates");
  await click(setupPage, "Show codes"); await click(freshPage, "Show codes");
  await setupPage.getByText("Security codes:", { exact: false }).waitFor({ timeout: 5000 }).catch(async error => { throw new Error(`First browser: ${await setupPage.getByRole("alert").allTextContents()} (${error.message})`); });
  await freshPage.getByText("Security codes:", { exact: false }).waitFor({ timeout: 5000 }).catch(async error => { throw new Error(`Second browser: ${await freshPage.getByRole("alert").allTextContents()} (${error.message})`); });
  const firstCodes = await setupPage.getByText("Security codes:", { exact: false }).textContent();
  const secondCodes = await freshPage.getByText("Security codes:", { exact: false }).textContent();
  assert.equal(firstCodes, secondCodes);
  await click(setupPage, "Authorize verified device");
  await setupPage.getByRole("alert").filter({ hasText: /verification first/i }).waitFor();
  await click(setupPage, "The codes match on both devices");
  await click(freshPage, "Receive updates"); await click(freshPage, "The codes match on both devices");
  await click(setupPage, "Receive updates"); await click(setupPage, "Send pending events");
  await click(freshPage, "Send pending events"); await click(freshPage, "Receive updates");
  await click(setupPage, "Receive updates"); await click(setupPage, "Authorize verified device");
  await setupPage.getByText("Device authorized after verification.").waitFor();
  assert.equal((await MatrixDevice.findOne({ userId: `@${secondAccount}:sonar.invalid`, deviceId: secondId })).serverAuthorization, "authorized");
  await click(setupPage, "Close verification");
  await click(freshPage, "Close verification");
  const revocation = await setupPage.evaluate(async ({ account, targetId }) => {
    const { listBrowserMatrixDevices, revokeBrowserMatrixDevice } = await import("/src/lib/matrixDeviceClient.js");
    const { axiosInstance: api } = await import("/src/lib/axios.js");
    const userId = `@${account}:sonar.invalid`, devices = await listBrowserMatrixDevices(userId, api);
    const target = devices.find(device => device.deviceId === targetId);
    let changedRejected = false;
    try { await revokeBrowserMatrixDevice(userId, { ...target, ed25519: "A".repeat(43) }, api); } catch { changedRejected = true; }
    return { changedRejected };
  }, { account: secondAccount, targetId: secondId });
  assert.equal(revocation.changedRejected, true);
  await setupPage.getByRole("button", { name: "Show devices" }).click();
  await setupPage.getByRole("button", { name: `Remove ${secondId}` }).click();
  await setupPage.getByRole("button", { name: "Confirm removal" }).click();
  await setupPage.getByRole("button", { name: `Remove ${secondId}` }).waitFor({ state: "detached" });
  assert.ok((await MatrixDevice.findOne({ userId: `@${secondAccount}:sonar.invalid`, deviceId: secondId })).revokedAt);
  assert.equal(await freshPage.evaluate(async () => { const r = await fetch("/api/matrix/device/list"); return r.status; }), 400);
  assert.equal(await freshPage.evaluate(async id => (await (await import("/src/lib/crypto.js")).existingIdentity(id)) === null, secondAccount), true);
  assert.equal((await User.findById(secondAccount)).encryptionKey, secondLegacy);
  await freshContext.close();
  await setupContext.close();
  console.log("PASS: separate browsers preserve legacy keys, exchange matching SAS codes, and authorize only after verification.");
} finally {
  await browser?.close();
  if (server) await new Promise(resolve => server.close(resolve));
  await vite?.close();
  await mongoose.disconnect(); await db.stop();
}
