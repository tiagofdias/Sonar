import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import bcrypt from "bcryptjs";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import { io as client } from "socket.io-client";
import { app, server, io } from "../lib/socket.js";
import authRoutes from "./auth.route.js";
import User from "../models/user.model.js";

test("account settings, password changes, and session revocation", async (t) => {
  const database = await MongoMemoryServer.create({
    instance: { ip: "127.0.0.1" },
  });
  const sockets = [];
  try {
    await mongoose.connect(database.getUri("sonar-auth-tests"));
    const pair = await crypto.subtle.generateKey(
      { name: "ECDH", namedCurve: "P-256" },
      true,
      ["deriveKey"],
    );
    const publicKey = Buffer.from(
      await crypto.subtle.exportKey("raw", pair.publicKey),
    ).toString("base64");
    const originalPassword = "original-test-password";
    const nextPassword = "TenChars!!";
    const user = await User.create({
      fullName: "Settings tester",
      email: "settings@example.invalid",
      password: await bcrypt.hash(originalPassword, 4),
      encryptionKey: publicKey,
    });
    app.use(express.json());
    app.use(cookieParser());
    app.use("/api/auth", authRoutes);
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    async function request(path, method = "GET", body, cookie) {
      const res = await fetch(url + "/api/auth" + path, {
        method,
        headers: {
          "Content-Type": "application/json",
          ...(cookie ? { Cookie: cookie } : {}),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return {
        status: res.status,
        body: await res.json(),
        cookie: res.headers.get("set-cookie")?.split(";")[0],
      };
    }
    async function connect(cookie, expectedEvent = "connect") {
      const socket = client(url, {
        autoConnect: false,
        reconnection: false,
        transports: ["websocket"],
        extraHeaders: { Cookie: cookie },
      });
      sockets.push(socket);
      await new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("Socket test timed out")),
          5000,
        );
        socket.once(expectedEvent, () => {
          clearTimeout(timer);
          resolve();
        });
        if (expectedEvent !== "connect_error")
          socket.once("connect_error", (error) => {
            clearTimeout(timer);
            reject(error);
          });
        socket.connect();
      });
      return socket;
    }
    let cookie, socket;
    await t.test("login, profile edits, and response fields", async () => {
      const login = await request("/login", "POST", {
        email: user.email,
        password: originalPassword,
      });
      assert.equal(login.status, 200);
      assert.equal(login.body.encryptionKey, publicKey);
      assert.equal(login.body.password, undefined);
      cookie = login.cookie;
      const result = await request(
        "/update-profile",
        "PUT",
        {
          fullName: "  New display name  ",
          email: "other@example.invalid",
          encryptionKey: "overwrite",
        },
        cookie,
      );
      assert.equal(result.status, 200);
      assert.equal(result.body.fullName, "New display name");
      assert.equal(result.body.email, user.email);
      assert.equal(result.body.encryptionKey, publicKey);
      assert.equal(result.body.password, undefined);
      assert.equal(result.body.sessionVersion, undefined);
      assert.equal(
        (await request("/update-profile", "PUT", { fullName: " " }, cookie))
          .status,
        400,
      );
      assert.equal(
        (await request("/check", "GET", undefined, "jwt=malformed")).status,
        401,
      );
      socket = await connect(cookie);
    });
    await t.test("identity registration rejects invalid points and preserves an existing key", async () => {
      const offCurve = Buffer.concat([Buffer.from([4]), Buffer.alloc(64)]).toString("base64");
      assert.equal((await request("/encryption-key", "PUT", { publicKey: offCurve }, cookie)).status, 400);
      const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
      const noncanonical = publicKey.slice(0, -2) + alphabet[alphabet.indexOf(publicKey.at(-2)) ^ 1] + "=";
      assert.equal(Buffer.from(noncanonical, "base64").toString("base64"), publicKey);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: noncanonical }, cookie)).status, 400);
      assert.equal((await request("/encryption-key", "PUT", { publicKey }, cookie)).status, 200);
      const other = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveKey"]);
      const replacement = Buffer.from(await crypto.subtle.exportKey("raw", other.publicKey)).toString("base64");
      assert.equal((await request("/encryption-key", "PUT", { publicKey: replacement }, cookie)).status, 409);
      assert.equal((await User.findById(user._id)).encryptionKey, publicKey);
    });
    await t.test("individual sessions are account-scoped and revocation disconnects only that login", async () => {
      const second = await request("/login", "POST", { email: user.email, password: originalPassword });
      assert.equal(second.status, 200);
      const secondSocket = await connect(second.cookie);
      const listing = await request("/sessions", "GET", undefined, cookie);
      assert.equal(listing.status, 200);
      assert.equal(listing.body.legacyCurrent, false);
      assert.equal(listing.body.sessions.length, 2);
      const current = listing.body.sessions.find(item => item.current);
      const other = listing.body.sessions.find(item => !item.current);
      assert.ok(current && other);
      assert.equal(other.method, "password");
      assert.equal(listing.body.sessions.some(item => JSON.stringify(item).includes(second.cookie)), false);
      const stranger = await User.create({ fullName: "Stranger", email: "session-stranger@example.invalid", password: "unused" });
      const strangerCookie = `jwt=${jwt.sign({ userId: String(stranger._id) }, process.env.JWT_SECRET)}`;
      assert.equal((await request(`/sessions/${other.id}`, "DELETE", undefined, strangerCookie)).status, 404);
      const revoked = new Promise(resolve => secondSocket.once("sessionRevoked", resolve));
      const disconnected = new Promise(resolve => secondSocket.once("disconnect", resolve));
      assert.equal((await request(`/sessions/${other.id}`, "DELETE", undefined, cookie)).status, 200);
      await Promise.all([revoked, disconnected]);
      assert.equal((await request("/check", "GET", undefined, second.cookie)).status, 401);
      assert.equal((await request("/check", "GET", undefined, cookie)).status, 200);
      assert.equal(socket.connected, true);
      await connect(second.cookie, "connect_error");
      const legacyCookie = `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET)}`;
      assert.equal((await request("/sessions", "GET", undefined, legacyCookie)).body.legacyCurrent, true);
    });
    await t.test(
      "password validation leaves identity and active session intact",
      async () => {
        assert.equal(
          (
            await request(
              "/password",
              "PUT",
              { currentPassword: originalPassword, newPassword: "short" },
              cookie,
            )
          ).status,
          400,
        );
        assert.equal(
          (
            await request(
              "/password",
              "PUT",
              {
                currentPassword: originalPassword,
                newPassword: "😀".repeat(20),
              },
              cookie,
            )
          ).status,
          400,
        );
        assert.equal(
          (
            await request(
              "/password",
              "PUT",
              {
                currentPassword: originalPassword,
                newPassword: originalPassword,
              },
              cookie,
            )
          ).status,
          400,
        );
        assert.equal(
          (
            await request(
              "/password",
              "PUT",
              { currentPassword: "incorrect", newPassword: nextPassword },
              cookie,
            )
          ).status,
          400,
        );
        assert.equal(
          (await request("/check", "GET", undefined, cookie)).status,
          200,
        );
      },
    );
    await t.test(
      "password change revokes cookies and sockets while preserving encryption keys",
      async () => {
        let revoked = false;
        socket.once("sessionRevoked", () => {
          revoked = true;
        });
        const disconnected = new Promise((resolve) =>
          socket.once("disconnect", resolve),
        );
        const result = await request(
          "/password",
          "PUT",
          { currentPassword: originalPassword, newPassword: nextPassword },
          cookie,
        );
        assert.equal(result.status, 200);
        await disconnected;
        assert.equal(revoked, true);
        assert.equal(
          (await request("/check", "GET", undefined, cookie)).status,
          401,
        );
        await connect(cookie, "connect_error");
        assert.equal(
          (
            await request("/login", "POST", {
              email: user.email,
              password: originalPassword,
            })
          ).status,
          400,
        );
        const changed = await User.findById(user._id);
        assert.equal(changed.encryptionKey, publicKey);
        assert.equal(changed.sessionVersion, 1);
        assert.equal(
          await bcrypt.compare(nextPassword, changed.password),
          true,
        );
        const login = await request("/login", "POST", {
          email: user.email,
          password: nextPassword,
        });
        assert.equal(login.status, 200);
        cookie = login.cookie;
        assert.equal(
          jwt.verify(cookie.slice(4), process.env.JWT_SECRET).sessionVersion,
          1,
        );
      },
    );
    await t.test(
      "logout revokes all cookies of the previous session version",
      async () => {
        const second = await request("/login", "POST", {
          email: user.email,
          password: nextPassword,
        });
        const otherSocket = await connect(second.cookie);
        const disconnected = new Promise((resolve) =>
          otherSocket.once("disconnect", resolve),
        );
        assert.equal(
          (await request("/logout", "POST", {}, cookie)).status,
          200,
        );
        await disconnected;
        assert.equal(
          (await request("/check", "GET", undefined, cookie)).status,
          401,
        );
        assert.equal(
          (await request("/check", "GET", undefined, second.cookie)).status,
          401,
        );
      },
    );
    await t.test("new-account password bounds and password-attempt limits", async () => {
      const email = "new-account@example.invalid";
      assert.equal((await request("/signup", "POST", { fullName: "New member", email, password: "short" })).status, 400);
      assert.equal((await request("/signup", "POST", { fullName: "New member", email, password: "😀".repeat(20) })).status, 400);
      const created = await request("/signup", "POST", { fullName: "New member", email, password: "a".repeat(72) });
      assert.equal(created.status, 201);
      assert.equal(created.body.password, undefined);
      assert.equal((await request("/login", "POST", { email: { $ne: null }, password: "anything" })).status, 400);
      for (let i = 0; i < 10; i++) assert.equal((await request("/password", "PUT", { currentPassword: "x", newPassword: "short" }, created.cookie)).status, 400);
      assert.equal((await request("/password", "PUT", { currentPassword: "x", newPassword: "short" }, created.cookie)).status, 429);
    });
    await t.test("login and signup endpoints enforce their own request limits", async () => {
      let login, signup;
      for (let i = 0; i < 31; i++) login = await request("/login", "POST", {});
      for (let i = 0; i < 11; i++) signup = await request("/signup", "POST", {});
      assert.equal(login.status, 429);
      assert.equal(signup.status, 429);
    });
  } finally {
    sockets.forEach((socket) => socket.disconnect());
    await new Promise((resolve) => io.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
