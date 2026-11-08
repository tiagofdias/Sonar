// Runs only against a freshly created in-memory MongoDB and synthetic account.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import bcrypt from "bcryptjs";
import { MongoMemoryServer } from "mongodb-memory-server";
import { io as socketClient } from "socket.io-client";
import { app, server, io } from "../src/lib/socket.js";
import authRoutes from "../src/routes/auth.route.js";
import User from "../src/models/user.model.js";

process.env.JWT_SECRET = "sonar-native-live-isolated-test-secret";
delete process.env.MONGO_SHARED_AUTH_LIMITS;
const database = await MongoMemoryServer.create({ instance: { ip: "127.0.0.1" } });
let child, caller;
try {
  await mongoose.connect(database.getUri("sonar-native-live-test"));
  const account = await User.create({ fullName: "Synthetic user", email: "native-live@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  const other = await User.create({ fullName: "Synthetic caller", email: "native-caller@example.invalid", password: await bcrypt.hash("synthetic-password", 4) });
  app.use(express.json());
  app.use(cookieParser());
  app.use("/api/auth", authRoutes);
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const login = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: account.email, password: "synthetic-password" }) });
  assert.equal(login.status, 200);
  const cookie = login.headers.get("set-cookie")?.split(";")[0];
  assert.ok(cookie?.startsWith("jwt="));
  const callerLogin = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: other.email, password: "synthetic-password" }) });
  assert.equal(callerLogin.status, 200);
  const callerCookie = callerLogin.headers.get("set-cookie")?.split(";")[0];
  assert.ok(callerCookie?.startsWith("jwt="));
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
  const cargo = process.platform === "win32" ? resolve(process.env.USERPROFILE, ".cargo/bin/cargo.exe") : "cargo";
  const args = ["+stable-x86_64-pc-windows-msvc", "test", "--manifest-path", "desktop/Cargo.toml", "live::tests::sonar_server_presence_and_account_revocation", "--", "--ignored", "--nocapture"];
  child = spawn(cargo, args, { cwd: root, windowsHide: true, env: { ...process.env, SONAR_NATIVE_TEST_BASE: base, SONAR_NATIVE_TEST_COOKIE: cookie, SONAR_NATIVE_TEST_USER: String(account._id) }, stdio: ["ignore", "pipe", "pipe"] });
  let output = "", errors = "", revocation, callFlow;
  child.stdout.on("data", chunk => {
    output += chunk.toString();
    if (!callFlow && output.includes("SONAR_NATIVE_TEST_READY")) {
      callFlow = (async () => {
        caller = socketClient(base, { autoConnect: false, reconnection: false, transports: ["websocket"], extraHeaders: { Cookie: callerCookie } });
        const connected = once(caller, "connect"); caller.connect(); await connected;
        const ended = once(caller, "call:ended");
        const answered = once(caller, "call:signal");
        const outgoing = new Promise(resolve => {
          const receive = payload => {
            if (payload.signal?.type === "offer") { caller.off("call:signal", receive); resolve(payload); }
          };
          caller.on("call:signal", receive);
        });
        const callId = "6bfa8180-b4fb-44f8-8922-6ad700b06b68";
        const ack = await new Promise((resolve, reject) => caller.timeout(5000).emit("call:signal", { to: String(account._id), signal: { type: "offer", callId, sdp: { type: "offer", sdp: "synthetic-sdp" } } }, (error, result) => error ? reject(error) : resolve(result)));
        assert.equal(ack.ok, true);
        const [reply] = await answered;
        assert.equal(reply.from, String(account._id));
        assert.equal(reply.signal.type, "answer");
        assert.equal(reply.signal.callId, callId);
        assert.equal(reply.signal.sdp.sdp, "synthetic-answer");
        const [event] = await ended;
        assert.equal(event.callId, callId);
        assert.equal(event.reason, "ended");
        const offered = await outgoing;
        assert.equal(offered.from, String(account._id));
        assert.equal(offered.signal.sdp.sdp, "synthetic-native-offer");
        const outgoingEnded = once(caller, "call:ended");
        const answerAck = await new Promise((resolve, reject) => caller.timeout(5000).emit("call:signal", { to: String(account._id), signal: { type: "answer", callId: offered.signal.callId, sdp: { type: "answer", sdp: "synthetic-web-answer" } } }, (error, result) => error ? reject(error) : resolve(result)));
        assert.equal(answerAck.ok, true);
        assert.equal((await outgoingEnded)[0].callId, offered.signal.callId);
      })();
      callFlow.catch(() => {});
    }
    if (!revocation && output.includes("SONAR_NATIVE_TEST_DECLINED")) {
      revocation = fetch(`${base}/api/auth/logout`, { method: "POST", headers: { Cookie: cookie } }).then(response => assert.equal(response.status, 200));
      revocation.catch(() => {});
    }
  });
  child.stderr.on("data", chunk => { errors += chunk.toString(); });
  const timeout = setTimeout(() => child.kill(), 60000);
  const [code] = await once(child, "exit");
  clearTimeout(timeout);
  assert.ok(revocation, `native client did not decline the call: ${errors.slice(-2000)} ${output.slice(-2000)}`);
  assert.ok(callFlow, "native client did not receive the call");
  await callFlow;
  await revocation;
  assert.equal(code, 0, `native integration test failed: ${errors.slice(-2000)} ${output.slice(-2000)}`);
  console.log("PASS: native Socket.IO signals incoming and outgoing calls through the isolated server and handles session revocation.");
} finally {
  caller?.disconnect();
  if (child && child.exitCode === null) child.kill();
  io.close();
  if (server.listening) await new Promise(resolve => server.close(resolve));
  await mongoose.disconnect();
  await database.stop();
}
