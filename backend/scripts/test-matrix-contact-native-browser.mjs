// Synthetic account, isolated database, browser and native SDK process only.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import express from "express";
import cookieParser from "cookie-parser";
import bcrypt from "bcryptjs";
import mongoose from "mongoose";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { chromium } from "playwright";
import User from "../src/models/user.model.js";
import LoginSession from "../src/models/loginSession.model.js";
import MatrixDevice from "../src/models/matrixDevice.model.js";
import MatrixRoomEvent from "../src/models/matrixRoomEvent.model.js";
import authRoutes from "../src/routes/auth.route.js";
import { createMatrixDeviceRouter } from "../src/routes/matrixDevice.route.js";
import { createMatrixMessagingRouter } from "../src/routes/matrixMessaging.route.js";
import { bootstrapMatrixDevice, authorizeMatrixDevice } from "../src/lib/matrixEnrollment.js";

process.env.JWT_SECRET = "synthetic-pairing-only-secret";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const temp = await mkdtemp(resolve(tmpdir(), "sonar-pairing-"));
const db = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
let server, vite, browser, child;
try {
  await mongoose.connect(db.getUri("pairing-test"));
  const account = await User.create({ fullName: "Synthetic", email: "pairing-test@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const nativeAccount = await User.create({ fullName: "Synthetic native", email: "native-contact-test@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const userId = `@${account._id}:sonar.invalid`;
  const app = express(); app.use(express.json()); app.use(cookieParser()); app.use("/api/auth", authRoutes);
  const routers = new Map();
  app.use("/api/matrix/device", (req, res, next) => {
    const key = req.cookies.jwt || "anonymous";
    if (!routers.has(key)) routers.set(key, createMatrixDeviceRouter("sonar.invalid"));
    routers.get(key)(req, res, next);
  });
  app.use("/api/matrix/messages", createMatrixMessagingRouter("sonar.invalid"));
  const requireFrontend = createRequire(resolve(root, "frontend/package.json"));
  const { createServer } = await import(pathToFileURL(requireFrontend.resolve("vite")).href);
  vite = await createServer({ root: resolve(root, "frontend"), configFile: resolve(root, "frontend/vite.config.js"), server: { middlewareMode: true, proxy: {} }, appType: "spa" });
  app.use(vite.middlewares);
  await new Promise(done => { server = app.listen(0, "127.0.0.1", done); });
  const base = `http://127.0.0.1:${server.address().port}`;
  const login = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: account.email, password: "synthetic-password" }) });
  assert.equal(login.status, 200);
  const cookie = login.headers.get("set-cookie").split(";")[0];
  browser = await chromium.launch({ headless: true, ...(process.env.SONAR_BROWSER_EXECUTABLE ? { executablePath: process.env.SONAR_BROWSER_EXECUTABLE } : {}) });
  const context = await browser.newContext();
  await context.addCookies([{ name: "jwt", value: cookie.slice(4), url: base, httpOnly: true, sameSite: "Strict" }]);
  const page = await context.newPage(); await page.goto(base);
  await page.evaluate(async user => {
    const { createBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { registerExistingBrowserMatrixDevice } = await import("/src/lib/matrixDeviceClient.js");
    const api = { async get(path, options = {}) { const query = new URLSearchParams(options.params || {}).toString(); const r = await fetch(`/api${path}${query ? `?${query}` : ""}`); if (!r.ok) throw Error(`GET ${r.status}`); return { data: await r.json() }; }, async post(path, body) { const r = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }); if (!r.ok) throw Error(`POST ${r.status}`); return { data: await r.json() }; } };
    await createBrowserMatrixIdentity(user, "BROWSER");
    await registerExistingBrowserMatrixDevice(user, api);
    window.sonarTestApi = api;
  }, userId);
  const browserSession = await LoginSession.findOne({ userId: account._id, "matrixDevice.deviceId": "BROWSER" }).lean();
  await bootstrapMatrixDevice(String(account._id), browserSession._id, "sonar.invalid", "BROWSER");
  await page.evaluate(async user => {
    const { publishForTest } = await import("/src/lib/matrixContactTestFixture.js");
    await publishForTest(user, window.sonarTestApi);
  }, userId);
  child = spawn(resolve(root, "desktop/target/debug/examples/matrix_contact_probe.exe"), [], { windowsHide: true, env: { ...process.env, SONAR_CONTACT_TEST_BASE: base, SONAR_CONTACT_TEST_ROOT: temp, SONAR_CONTACT_TEST_PEER: String(account._id) }, stdio: ["pipe", "pipe", "pipe"] });
  const nativeExit = once(child, "exit");
  let errors = ""; child.stderr.on("data", chunk => { errors += chunk; });
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  async function next() { let timer; try { const result = await Promise.race([lines.next(), new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`Native timeout: ${errors}`)), 20000); })]); if (result.done) throw Error(`Native stopped: ${errors}`); return JSON.parse(result.value); } finally { clearTimeout(timer); } }
  async function command(value) { child.stdin.write(`${value}\n`); return next(); }
  assert.equal((await next()).type, "registered");
  const nativeSession = await LoginSession.findOne({ userId: nativeAccount._id, "matrixDevice.deviceId": "NATIVECONTACT" }).lean();
  await bootstrapMatrixDevice(String(nativeAccount._id), nativeSession._id, "sonar.invalid", "NATIVECONTACT");
  await page.evaluate(async ({ userId, peer }) => {
    const { prepareBrowserDirectMatrixRoom } = await import("/src/lib/matrixDeviceClient.js");
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { prepareContactVerification } = await import("/src/lib/matrixContactVerification.js");
    const room = await prepareBrowserDirectMatrixRoom(userId, peer, window.sonarTestApi);
    window.sonarReady = new Promise((resolve, reject) => {
      withBrowserMatrixIdentity(userId, (machine, descriptor) => new Promise(async done => {
        try {
          window.sonarFlow = await prepareContactVerification(machine, descriptor, room, `@${peer}:sonar.invalid`, "NATIVECONTACT", window.sonarTestApi);
          window.sonarClose = done; resolve();
        } catch (error) { done(); reject(error); }
      })).catch(reject);
    });
    await window.sonarReady;
  }, { userId, peer: String(nativeAccount._id) });
  const browserStarts = process.env.SONAR_TEST_BROWSER_INITIATES === "true";
  if (browserStarts) {
    assert.equal((await command("listen")).type, "listening");
    await page.evaluate(async () => { await window.sonarFlow.start(); });
    assert.equal((await command("check")).type, "started");
    await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.startSas(); });
    await command("poll");
  } else {
    assert.equal((await command("start")).type, "started");
    await page.evaluate(async () => { await window.sonarFlow.receive(); const request = window.sonarFlow.incoming()[0]; if (!request) throw Error("Native request missing"); await window.sonarFlow.accept(request.flowId); });
    await command("poll");
    await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.acceptSas(); });
  }
  let nativeCodes, browserCodes;
  for (let i = 0; i < 8; i++) {
    const state = await command("poll"); nativeCodes = state.codes;
    browserCodes = await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.flush(); try { return window.sonarFlow.codes(); } catch { return null; } });
    if (nativeCodes && browserCodes) break;
  }
  assert.deepEqual(nativeCodes, browserCodes);
  assert.equal(nativeCodes?.length, 3);
  await command("confirm");
  await page.evaluate(async () => { await window.sonarFlow.confirm(true); });
  let done = false;
  for (let i = 0; i < 8; i++) {
    const state = await command("poll");
    await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.flush(); });
    if (state.done) { done = true; break; }
  }
  assert.equal(done, true);
  await page.evaluate(async () => { await window.sonarFlow.receive(); if (!window.sonarFlow.verified()) throw Error("Browser did not verify contact"); window.sonarClose(); });
  const sent = await command("send");
  assert.equal(sent.type, "sent");
  const history = await page.evaluate(async ({ account, peer }) => {
    const { readDirectMatrixHistory } = await import("/src/lib/matrixDirectClient.js");
    return readDirectMatrixHistory(account, peer, window.sonarTestApi);
  }, { account: String(account._id), peer: String(nativeAccount._id) });
  const message = history.messages.find(item => item.id === sent.id);
  assert.equal(message?.text, "synthetic verified native message");
  assert.equal(message?.unreadable, false);
  assert.equal(message?.replyTo, "0123456789abcdef01234567");
  const encryptedRecord = await MatrixRoomEvent.findById(sent.id).lean();
  assert.equal(Object.hasOwn(encryptedRecord.content, "m.relates_to"), false);
  const reacted = await command("react");
  const reactionHistory = await page.evaluate(async ({ account, peer }) => {
    const { readDirectMatrixHistory } = await import("/src/lib/matrixDirectClient.js");
    return readDirectMatrixHistory(account, peer, window.sonarTestApi);
  }, { account: String(account._id), peer: String(nativeAccount._id) });
  assert.deepEqual(reactionHistory.messages.find(item => item.id === reacted.id)?.reaction, { target: sent.id, emoji: "👍" });
  const encryptedReaction = await MatrixRoomEvent.findById(reacted.id).lean();
  assert.equal(Object.hasOwn(encryptedReaction.content, "m.relates_to"), false);
  assert.equal((await command("unreact")).type, "unreacted");
  const afterRemoval = await page.evaluate(async ({ account, peer }) => {
    const { readDirectMatrixHistory } = await import("/src/lib/matrixDirectClient.js");
    return readDirectMatrixHistory(account, peer, window.sonarTestApi);
  }, { account: String(account._id), peer: String(nativeAccount._id) });
  assert.equal(afterRemoval.messages.some(item => item.id === reacted.id), false);
  const browserSent = await page.evaluate(async ({ account, peer }) => {
    const { sendDirectMatrixText } = await import("/src/lib/matrixDirectClient.js");
    return sendDirectMatrixText(account, peer, "synthetic verified browser message", window.sonarTestApi);
  }, { account: String(account._id), peer: String(nativeAccount._id) });
  assert.equal((await command("read")).type, "read");
  const secondLogin = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: account.email, password: "synthetic-password" }) });
  const secondCookie = secondLogin.headers.get("set-cookie").split(";")[0];
  const secondContext = await browser.newContext();
  await secondContext.addCookies([{ name: "jwt", value: secondCookie.slice(4), url: base, httpOnly: true, sameSite: "Strict" }]);
  const secondPage = await secondContext.newPage(); await secondPage.goto(base);
  await secondPage.evaluate(async user => {
    const { createBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { registerExistingBrowserMatrixDevice } = await import("/src/lib/matrixDeviceClient.js");
    const api = { async get(path, options = {}) { const query = new URLSearchParams(options.params || {}).toString(); const r = await fetch(`/api${path}${query ? `?${query}` : ""}`); if (!r.ok) throw Error(`GET ${r.status}`); return { data: await r.json() }; }, async post(path, body) { const r = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }); if (!r.ok) throw Error(`POST ${r.status}`); return { data: await r.json() }; } };
    await createBrowserMatrixIdentity(user, "REMOVED"); await registerExistingBrowserMatrixDevice(user, api);
  }, userId);
  const removed = await MatrixDevice.findOne({ userId, deviceId: "REMOVED" }).lean();
  // Fixture-only setup: SAS approval is exercised separately in the pairing test.
  await authorizeMatrixDevice(String(account._id), browserSession._id, "sonar.invalid", "BROWSER", "REMOVED", removed.deviceKeys.keys["ed25519:REMOVED"]);
  await page.evaluate(async user => {
    const { listBrowserMatrixDevices, revokeBrowserMatrixDevice } = await import("/src/lib/matrixDeviceClient.js");
    const target = (await listBrowserMatrixDevices(user, window.sonarTestApi)).find(device => device.deviceId === "REMOVED");
    await revokeBrowserMatrixDevice(user, target, window.sonarTestApi);
  }, userId);
  const newSent = await page.evaluate(async ({ account, peer }) => {
    const { sendDirectMatrixText } = await import("/src/lib/matrixDirectClient.js");
    return sendDirectMatrixText(account, peer, "browser message after revocation", window.sonarTestApi);
  }, { account: String(account._id), peer: String(nativeAccount._id) });
  const beforeEvent = await MatrixRoomEvent.findById(browserSent.id).lean(), afterEvent = await MatrixRoomEvent.findById(newSent.id).lean();
  assert.notEqual(beforeEvent.content.session_id, afterEvent.content.session_id);
  assert.equal((await command("read-rotated")).type, "read-rotated");
  await secondContext.close();
  assert.equal((await nativeExit)[0], 0);
  console.log("PASS: browser/native verified messaging and cross-platform key rotation after device revocation.");
} finally {
  if (child && child.exitCode === null) child.kill();
  await browser?.close(); await vite?.close();
  if (server) await new Promise(done => server.close(done));
  await mongoose.disconnect(); await db.stop();
  if (resolve(temp).startsWith(resolve(tmpdir()) + sep) && dirname(resolve(temp)) === resolve(tmpdir())) await rm(temp, { recursive: true, force: true });
}
