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
import authRoutes from "../src/routes/auth.route.js";
import { createMatrixDeviceRouter } from "../src/routes/matrixDevice.route.js";
import { bootstrapMatrixDevice } from "../src/lib/matrixEnrollment.js";

process.env.JWT_SECRET = "synthetic-pairing-only-secret";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const reverse = process.env.SONAR_PAIR_TEST_REVERSE === "1";
const temp = await mkdtemp(resolve(tmpdir(), "sonar-pairing-"));
const db = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
let server, vite, browser, child;
try {
  await mongoose.connect(db.getUri("pairing-test"));
  const account = await User.create({ fullName: "Synthetic", email: "pairing-test@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const userId = `@${account._id}:sonar.invalid`;
  const app = express(); app.use(express.json()); app.use(cookieParser()); app.use("/api/auth", authRoutes);
  const routers = new Map();
  app.use("/api/matrix/device", (req, res, next) => {
    const key = req.cookies.jwt || "anonymous";
    if (!routers.has(key)) routers.set(key, createMatrixDeviceRouter("sonar.invalid"));
    routers.get(key)(req, res, next);
  });
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
  if (!reverse) await bootstrapMatrixDevice(String(account._id), browserSession._id, "sonar.invalid", "BROWSER");
  await page.evaluate(async user => {
    const { withBrowserMatrixVerification } = await import("/src/lib/matrixVerificationSession.js");
    window.sonarReady = new Promise((resolve, reject) => {
      withBrowserMatrixVerification(user, window.sonarTestApi, session => new Promise(done => { window.sonarSession = session; window.sonarClose = done; resolve(); })).catch(reject);
    });
    await window.sonarReady;
  }, userId);
  child = spawn(resolve(root, "desktop/target/debug/examples/matrix_pairing_probe.exe"), [], { windowsHide: true, env: { ...process.env, SONAR_PAIR_TEST_BASE: base, SONAR_PAIR_TEST_ROOT: temp, SONAR_PAIR_TEST_TARGET: "BROWSER", ...(reverse ? { SONAR_PAIR_TEST_REVERSE: "1" } : {}) }, stdio: ["pipe", "pipe", "pipe"] });
  let errors = ""; child.stderr.on("data", chunk => { errors += chunk; });
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  async function next() { let timer; try { const result = await Promise.race([lines.next(), new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`Native timeout: ${errors}`)), 20000); })]); if (result.done) throw Error(`Native stopped: ${errors}`); return JSON.parse(result.value); } finally { clearTimeout(timer); } }
  async function command(value) { child.stdin.write(`${value}\n`); return next(); }
  assert.equal((await next()).type, reverse ? "listening" : "started");
  if (reverse) {
    const nativeSession = await LoginSession.findOne({ userId: account._id, "matrixDevice.deviceId": "NATIVEPAIR" }).lean();
    await bootstrapMatrixDevice(String(account._id), nativeSession._id, "sonar.invalid", "NATIVEPAIR");
    await page.evaluate(async () => { await window.sonarSession.refresh(); window.sonarFlow = await window.sonarSession.createFlow("NATIVEPAIR"); await window.sonarFlow.start(); });
    assert.equal((await command("accept")).type, "started");
    await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.startSas(); });
    await command("poll");
  } else await page.evaluate(async () => {
    const requests = await window.sonarSession.receive();
    const request = requests.find(item => item.deviceId === "NATIVEPAIR");
    if (!request) throw Error("Native request missing");
    window.sonarFlow = await window.sonarSession.createFlow(request.deviceId);
    await window.sonarFlow.accept(request.flowId);
  });
  if (!reverse) { await command("poll"); await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.acceptSas(); }); }
  let nativeCodes, browserCodes;
  for (let i = 0; i < 8; i++) {
    const state = await command("poll"); nativeCodes = state.codes;
    browserCodes = await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.flush(); try { return window.sonarFlow.codes(); } catch { return null; } });
    if (nativeCodes && browserCodes) break;
  }
  assert.deepEqual(nativeCodes, browserCodes);
  assert.equal(nativeCodes?.length, 3);
  const prematureRejected = await page.evaluate(async () => { try { await window.sonarFlow.approve(); return false; } catch { return true; } });
  assert.equal(prematureRejected, true);
  assert.equal((await MatrixDevice.findOne({ userId, deviceId: reverse ? "BROWSER" : "NATIVEPAIR" }).lean()).serverAuthorization, "pending");
  await command("confirm");
  await page.evaluate(async () => { await window.sonarFlow.confirmCodesMatch(true); });
  let done = false;
  for (let i = 0; i < 8; i++) {
    const state = await command("poll");
    await page.evaluate(async () => { await window.sonarFlow.receive(); await window.sonarFlow.flush(); });
    if (state.done) { done = true; break; }
  }
  assert.equal(done, true);
  await page.evaluate(async () => { await window.sonarFlow.receive(); });
  if (reverse) await command("approve"); else await page.evaluate(async () => { await window.sonarFlow.approve(); });
  await page.evaluate(() => window.sonarClose());
  assert.equal((await MatrixDevice.findOne({ userId, deviceId: reverse ? "BROWSER" : "NATIVEPAIR" }).lean()).serverAuthorization, "authorized");
  child.stdin.write("close\n");
  assert.equal((await once(child, "exit"))[0], 0);
  console.log(`PASS: browser and native devices compared matching codes and authorized the ${reverse ? "browser" : "native"} device.`);
} finally {
  if (child && child.exitCode === null) child.kill();
  await browser?.close(); await vite?.close();
  if (server) await new Promise(done => server.close(done));
  await mongoose.disconnect(); await db.stop();
  if (resolve(temp).startsWith(resolve(tmpdir()) + sep) && dirname(resolve(temp)) === resolve(tmpdir())) await rm(temp, { recursive: true, force: true });
}
