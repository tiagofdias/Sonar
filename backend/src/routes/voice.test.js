import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac, randomUUID } from "node:crypto";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import { io as client } from "socket.io-client";
import { app, server, io } from "../lib/socket.js";
import voiceRoutes, { voiceConfiguration } from "./voice.route.js";
import User from "../models/user.model.js";
import { befriend } from "../lib/friends.js";
test("TURN config uses expiring anonymous credentials and fails closed", () => {
  const secret = "test-only-shared-secret",
    now = 1700000000000;
  const config = voiceConfiguration(
    {
      TURN_URLS: "turn:relay.example.invalid:3478",
      TURN_SHARED_SECRET: secret,
      VOICE_RELAY_ONLY: "true",
    },
    now,
  );
  assert.equal(config.turnConfigured, true);
  assert.equal(config.relayOnly, true);
  assert.equal(
    config.iceServers[0].username.split(":")[0],
    String(now / 1000 + 3600),
  );
  assert.equal(
    config.iceServers[0].credential,
    createHmac("sha1", secret)
      .update(config.iceServers[0].username)
      .digest("base64"),
  );
  assert.ok(!JSON.stringify(config).includes(secret));
  assert.deepEqual(voiceConfiguration({}).iceServers, []);
  assert.throws(() => voiceConfiguration({ VOICE_RELAY_ONLY: "true" }));
  assert.throws(() =>
    voiceConfiguration({ TURN_URLS: "turn:relay.example.invalid" }),
  );
  assert.throws(() =>
    voiceConfiguration({ STUN_URLS: "https://not-a-stun-server.invalid" }),
  );
});
test("authenticated direct-call signaling scopes call IDs, busy state, and cleanup", async (t) => {
  const database = await MongoMemoryServer.create();
  const sockets = [];
  try {
    await mongoose.connect(database.getUri("sonar-voice-tests"));
    const [alice, bob, outsider] = await User.create(
      ["Alice", "Bob", "Outsider"].map((fullName, i) => ({
        fullName,
        email: `voice-${i}@example.invalid`,
        password: "unused-test-hash",
      })),
    );
    await befriend(alice._id, bob._id);
    await befriend(alice._id, outsider._id);
    await befriend(bob._id, outsider._id);
    app.use(cookieParser());
    app.use("/api/voice", voiceRoutes);
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    const cookie = (user) =>
      `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}`;
    const connect = async (user) => {
      const socket = client(url, {
        transports: ["websocket"],
        reconnection: false,
        autoConnect: false,
        extraHeaders: { Cookie: cookie(user) },
      });
      sockets.push(socket);
      await new Promise((resolve, reject) => {
        socket.once("connect", resolve);
        socket.once("connect_error", reject);
        socket.connect();
      });
      return socket;
    };
    const a = await connect(alice),
      b = await connect(bob),
      c = await connect(outsider);
    const send = (socket, to, signal) =>
      new Promise((resolve, reject) =>
        socket
          .timeout(3000)
          .emit(
            "call:signal",
            { to: String(to._id), signal },
            (error, result) => (error ? reject(error) : resolve(result)),
          ),
      );
    const next = (socket, event) =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error(`Timed out waiting for ${event}`)),
          3000,
        );
        socket.once(event, (value) => {
          clearTimeout(timer);
          resolve(value);
        });
      });
    const callId = randomUUID();
    await t.test("voice configuration requires a valid session", async () => {
      assert.equal((await fetch(url + "/api/voice/config")).status, 401);
      const result = await fetch(url + "/api/voice/config", {
        headers: { Cookie: cookie(alice) },
      });
      assert.equal(result.status, 200);
      assert.equal(result.headers.get("cache-control"), "no-store");
    });
    await t.test(
      "offer reaches only the recipient, and busy/outsider signals are rejected",
      async () => {
        const incoming = next(b, "call:signal");
        assert.equal(
          (
            await send(a, bob, {
              type: "offer",
              callId,
              sdp: { type: "offer", sdp: "test-offer" },
            })
          ).ok,
          true,
        );
        assert.equal((await incoming).from, String(alice._id));
        assert.ok(
          (
            await send(c, bob, {
              type: "offer",
              callId: randomUUID(),
              sdp: { type: "offer", sdp: "busy" },
            })
          ).error,
        );
        assert.ok((await send(c, bob, { type: "hangup", callId })).error);
        assert.ok(
          (
            await send(a, outsider, {
              type: "ice",
              callId,
              candidate: { candidate: "test" },
            })
          ).error,
        );
      },
    );
    await t.test(
      "answer and candidate route to the active peer; hangup clears busy state",
      async () => {
        const answer = next(a, "call:signal");
        assert.equal(
          (
            await send(b, alice, {
              type: "answer",
              callId,
              sdp: { type: "answer", sdp: "test-answer" },
            })
          ).ok,
          true,
        );
        assert.equal((await answer).signal.callId, callId);
        const ice = next(b, "call:signal");
        assert.equal(
          (
            await send(a, bob, {
              type: "ice",
              callId,
              candidate: { candidate: "candidate:test" },
            })
          ).ok,
          true,
        );
        assert.equal((await ice).signal.type, "ice");
        const endedA = next(a, "call:ended"),
          endedB = next(b, "call:ended");
        assert.equal((await send(a, bob, { type: "hangup", callId })).ok, true);
        assert.equal((await endedA).callId, callId);
        assert.equal((await endedB).callId, callId);
        assert.ok(
          (
            await send(b, alice, {
              type: "answer",
              callId,
              sdp: { type: "answer", sdp: "stale" },
            })
          ).error,
        );
      },
    );
    await t.test(
      "a second caller waits, and can be declined or swapped to",
      async () => {
        const first = randomUUID();
        const offer = (id) => ({ type: "offer", callId: id, sdp: { type: "offer", sdp: "x" } });
        const answer = (id) => ({ type: "answer", callId: id, sdp: { type: "answer", sdp: "y" } });
        let arrival = next(b, "call:signal");
        assert.equal((await send(a, bob, offer(first))).ok, true);
        await arrival;
        arrival = next(a, "call:signal");
        assert.equal((await send(b, alice, answer(first))).ok, true);
        await arrival;
        const declined = randomUUID();
        arrival = next(b, "call:signal");
        assert.equal((await send(c, bob, offer(declined))).ok, true);
        assert.equal((await arrival).signal.callId, declined);
        await send(c, bob, { type: "hangup", callId: declined });
        const replacement = randomUUID();
        arrival = next(b, "call:signal");
        assert.equal((await send(c, bob, offer(replacement))).ok, true);
        await arrival;
        const firstEnded = next(a, "call:ended");
        assert.equal((await send(b, outsider, answer(replacement))).ok, true);
        assert.equal((await firstEnded).callId, first);
        assert.equal((await send(b, outsider, { type: "hangup", callId: replacement })).ok, true);
      },
    );
    await t.test(
      "disconnect ends the peer call and releases the account",
      async () => {
        const nextCall = randomUUID(),
          incoming = next(b, "call:signal");
        assert.equal(
          (
            await send(a, bob, {
              type: "offer",
              callId: nextCall,
              sdp: { type: "offer", sdp: "second-call" },
            })
          ).ok,
          true,
        );
        await incoming;
        const ended = next(b, "call:ended");
        a.disconnect();
        assert.equal((await ended).reason, "peerDisconnected");
        const lastCall = randomUUID(),
          lastIncoming = next(b, "call:signal");
        assert.equal(
          (
            await send(c, bob, {
              type: "offer",
              callId: lastCall,
              sdp: { type: "offer", sdp: "third-call" },
            })
          ).ok,
          true,
        );
        await lastIncoming;
        await send(b, outsider, { type: "hangup", callId: lastCall });
      },
    );
  } finally {
    sockets.forEach((socket) => socket.disconnect());
    await new Promise((resolve) => io.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});

test("screen-share signals are validated before relay", async () => {
  const { validScreenSignal } = await import("../lib/calls.js");
  assert.equal(validScreenSignal({ type: "screen-stop" }), true);
  assert.equal(validScreenSignal({ type: "screen-offer", sdp: { type: "offer", sdp: "v=0" } }), true);
  assert.equal(validScreenSignal({ type: "screen-offer", sdp: { type: "answer", sdp: "v=0" } }), false);
  assert.equal(validScreenSignal({ type: "screen-answer", sdp: { type: "answer", sdp: "v=0" } }), true);
  assert.equal(validScreenSignal({ type: "screen-ice", role: "viewer", candidate: { candidate: "x" } }), true);
  assert.equal(validScreenSignal({ type: "screen-ice", role: "other", candidate: { candidate: "x" } }), false);
  assert.equal(validScreenSignal({ type: "screen-unknown" }), false);
  // TV shares: a channel name for viewers, and desktop viewers saying so; nothing else.
  assert.equal(validScreenSignal({ type: "screen-offer", tv: true, label: "Sport TV 1", sdp: { type: "offer", sdp: "v=0" } }), true);
  assert.equal(validScreenSignal({ type: "screen-answer", client: "desktop", sdp: { type: "answer", sdp: "v=0" } }), true);
  assert.equal(validScreenSignal({ type: "screen-offer", label: "<img src=x>", sdp: { type: "offer", sdp: "v=0" } }), false);
  assert.equal(validScreenSignal({ type: "screen-offer", label: "x".repeat(81), sdp: { type: "offer", sdp: "v=0" } }), false);
  assert.equal(validScreenSignal({ type: "screen-offer", tv: "yes", sdp: { type: "offer", sdp: "v=0" } }), false);
  assert.equal(validScreenSignal({ type: "screen-answer", client: "evil", sdp: { type: "answer", sdp: "v=0" } }), false);
});
