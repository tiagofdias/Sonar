import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { io as client } from "socket.io-client";
import { app, server, io } from "../lib/socket.js";
import communityRoutes from "./community.route.js";
import Community from "../models/community.model.js";
import User from "../models/user.model.js";
test("voice-room membership, capacity, session routing, and revocation", async (t) => {
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1, ip: "127.0.0.1" } }),
    sockets = [];
  try {
    await mongoose.connect(database.getUri("sonar-room-tests"));
    const users = await User.create(
      Array.from({ length: 8 }, (_, i) => ({
        fullName: `Room member ${i}`,
        email: `room-${i}@example.invalid`,
        password: "unused-test-hash",
      })),
    );
    const [alice, bob, outsider] = users;
    const community = await Community.create({
      name: "Voice crew",
      ownerId: alice._id,
      members: users
        .filter((user) => user !== outsider)
        .map((user) => user._id),
      channels: [
        { name: "general", type: "text" },
        { name: "lounge", type: "voice" },
      ],
    });
    const roomId = String(community.channels[1]._id),
      communityId = String(community._id);
    app.use(express.json());
    app.use(cookieParser());
    app.use("/communities", communityRoutes);
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    const cookie = (user) =>
      `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}`;
    const connect = async (user) => {
      const socket = client(url, {
        autoConnect: false,
        reconnection: false,
        transports: ["websocket"],
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
    const send = (socket, event, payload) =>
      new Promise((resolve, reject) =>
        socket
          .timeout(3000)
          .emit(event, payload, (error, data) =>
            error ? reject(error) : resolve(data),
          ),
      );
    const next = (socket, event) =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error(`Missing ${event}`)),
          3000,
        );
        socket.once(event, (data) => {
          clearTimeout(timer);
          resolve(data);
        });
      });
    const a = await connect(alice),
      b = await connect(bob),
      c = await connect(outsider),
      payload = { communityId, channelId: roomId };
    let joinedA, joinedB;
    await t.test(
      "only members join voice channels and one session per account",
      async () => {
        assert.ok((await send(c, "voice:room-join", payload)).error);
        assert.ok(
          (
            await send(a, "voice:room-join", {
              communityId,
              channelId: String(community.channels[0]._id),
            })
          ).error,
        );
        joinedA = await send(a, "voice:room-join", payload);
        assert.equal(joinedA.participants.length, 1);
        joinedB = await send(b, "voice:room-join", payload);
        assert.equal(joinedB.participants.length, 2);
        const otherTab = await connect(bob);
        assert.ok((await send(otherTab, "voice:room-join", payload)).error);
        assert.ok((await send(c, "voice:rooms", { communityId })).error);
        assert.equal(
          (await send(a, "voice:rooms", { communityId })).rooms[0].participants
            .length,
          2,
        );
      },
    );
    await t.test("room locking and disconnect controls enforce roles and session IDs", async () => {
      const lock = (user, locked, channelId = roomId) => fetch(`${url}/communities/${communityId}/channels/${channelId}/voice-lock`, {
        method: "PATCH", headers: { Cookie: cookie(user), "Content-Type": "application/json" }, body: JSON.stringify({ locked }),
      });
      assert.equal((await lock(bob, true)).status, 403);
      assert.equal((await lock(alice, true, String(community.channels[0]._id))).status, 404);
      assert.equal((await lock(alice, true)).status, 200);
      assert.equal((await Community.findById(communityId)).channels.id(roomId).locked, true);
      assert.equal((await send(b, "voice:room-join", payload)).sessionId, joinedB.sessionId);
      const d = await connect(users[3]);
      assert.ok((await send(d, "voice:room-join", payload)).error);
      assert.ok((await send(b, "voice:room-kick", { ...payload, to: String(alice._id), toSession: joinedA.sessionId })).error);
      assert.ok((await send(a, "voice:room-kick", { ...payload, to: String(bob._id), toSession: "stale" })).error);
      const kicked = next(b, "voice:room-left");
      assert.equal((await send(a, "voice:room-kick", { ...payload, to: String(bob._id), toSession: joinedB.sessionId })).ok, true);
      assert.equal((await kicked).reason, "kicked");
      assert.ok((await send(b, "voice:room-join", payload)).error);
      assert.equal((await lock(alice, false)).status, 200);
      joinedB = await send(b, "voice:room-join", payload);
      assert.ok(joinedB.sessionId);
      const role = await fetch(`${url}/communities/${communityId}/members/${bob._id}/moderator`, {
        method: "PUT", headers: { Cookie: cookie(alice), "Content-Type": "application/json" }, body: JSON.stringify({ moderator: true }),
      });
      assert.equal(role.status, 200);
      assert.ok((await send(b, "voice:room-kick", { ...payload, to: String(alice._id), toSession: joinedA.sessionId })).error);
      const joinedD = await send(d, "voice:room-join", payload);
      const left = next(d, "voice:room-left");
      assert.equal((await send(b, "voice:room-kick", { ...payload, to: String(users[3]._id), toSession: joinedD.sessionId })).ok, true);
      assert.equal((await left).reason, "kicked");
      assert.equal((await lock(bob, true)).status, 200);
      assert.equal((await lock(bob, false)).status, 200);
      const demote = await fetch(`${url}/communities/${communityId}/members/${bob._id}/moderator`, {
        method: "PUT", headers: { Cookie: cookie(alice), "Content-Type": "application/json" }, body: JSON.stringify({ moderator: false }),
      });
      assert.equal(demote.status, 200);
      assert.equal((await lock(bob, true)).status, 403);
      d.disconnect();
    });
    await t.test(
      "signals bind both member sessions and status broadcasts update",
      async () => {
        const received = next(b, "voice:room-signal");
        const signal = {
          ...payload,
          sessionId: joinedA.sessionId,
          to: String(bob._id),
          toSession: joinedB.sessionId,
          signal: { type: "offer", sdp: { type: "offer", sdp: "test-offer" } },
        };
        assert.equal((await send(a, "voice:room-signal", signal)).ok, true);
        assert.equal((await received).from, String(alice._id));
        assert.ok((await send(c, "voice:room-signal", signal)).error);
        assert.ok(
          (
            await send(a, "voice:room-signal", {
              ...signal,
              toSession: "stale",
            })
          ).error,
        );
        assert.equal(
          (await send(b, "voice:room-status", { sessionId: joinedB.sessionId, muted: true, deafened: true }))
            .ok,
          true,
        );
        const state = (await send(a, "voice:rooms", { communityId })).rooms[0];
        assert.equal(
          state.participants.find((person) => person.userId === String(bob._id))
            .deafened,
          true,
        );
        const left = next(b, "voice:room-left");
        await send(b, "voice:room-leave", { sessionId: joinedB.sessionId });
        await left;
        const rejoined = await send(b, "voice:room-join", payload);
        assert.notEqual(rejoined.sessionId, joinedB.sessionId);
        assert.ok((await send(b, "voice:room-leave", { sessionId: joinedB.sessionId })).error);
        assert.ok((await send(b, "voice:room-leave", {})).error);
        assert.ok((await send(b, "voice:room-status", { sessionId: joinedB.sessionId, muted: true, deafened: true })).error);
        const active = (await send(a, "voice:rooms", { communityId })).rooms[0];
        assert.equal(active.participants.find(person => person.userId === String(bob._id)).sessionId, rejoined.sessionId);
        assert.equal(active.participants.find(person => person.userId === String(bob._id)).muted, false);
        assert.ok((await send(a, "voice:room-signal", signal)).error);
        joinedB = rejoined;
      },
    );
    await t.test("someone in a room can be called, but cannot place a call", async () => {
      const offer = () => ({ type: "offer", callId: crypto.randomUUID(), sdp: { type: "offer", sdp: "x" } });
      const first = offer();
      assert.equal((await send(c, "call:signal", { to: String(alice._id), signal: first })).ok, true);
      assert.ok((await send(c, "call:signal", { to: String(bob._id), signal: offer() })).error, "one call at a time per caller");
      assert.equal((await send(c, "call:signal", { to: String(alice._id), signal: { type: "hangup", callId: first.callId } })).ok, true);
      assert.ok((await send(a, "call:signal", { to: String(bob._id), signal: offer() })).error, "people in a room cannot start a call");
    });
    await t.test(
      "ban immediately ejects a room member and blocks rejoin",
      async () => {
        const removed = next(b, "voice:room-left");
        const response = await fetch(
          `${url}/communities/${communityId}/members/${bob._id}/ban`,
          { method: "POST", headers: { Cookie: cookie(alice) } },
        );
        assert.equal(response.status, 200);
        assert.equal((await removed).reason, "removed");
        assert.ok((await send(b, "voice:room-join", payload)).error);
        assert.equal(
          (await send(a, "voice:rooms", { communityId })).rooms[0].participants
            .length,
          1,
        );
      },
    );
    await t.test(
      "six-person capacity, disconnect release, and owner-only creation",
      async () => {
        const created = await fetch(
          `${url}/communities/${communityId}/channels`,
          {
            method: "POST",
            headers: {
              Cookie: cookie(alice),
              "Content-Type": "application/json",
            },
            body: JSON.stringify({ name: "music", type: "voice" }),
          },
        );
        assert.equal(created.status, 201);
        const denied = await fetch(
          `${url}/communities/${communityId}/channels`,
          {
            method: "POST",
            headers: {
              Cookie: cookie(users[3]),
              "Content-Type": "application/json",
            },
            body: JSON.stringify({ name: "not-allowed", type: "voice" }),
          },
        );
        assert.equal(denied.status, 403);
        const added = [];
        for (const user of users.slice(3)) {
          const socket = await connect(user);
          added.push(socket);
          const result = await send(socket, "voice:room-join", payload);
          assert.ok(result.sessionId);
        }
        assert.equal(
          (await send(a, "voice:rooms", { communityId })).rooms[0].participants
            .length,
          6,
        );
        const extra = await User.create({
          fullName: "Extra",
          email: "extra@example.invalid",
          password: "unused-test-hash",
        });
        await Community.updateOne(
          { _id: communityId },
          { $push: { members: extra._id } },
        );
        const extraSocket = await connect(extra);
        assert.ok((await send(extraSocket, "voice:room-join", payload)).error);
        added[0].disconnect();
        await new Promise((resolve) => setTimeout(resolve, 50));
        assert.ok(
          (await send(extraSocket, "voice:room-join", payload)).sessionId,
        );
        const textResult = await fetch(
          `${url}/communities/${communityId}/channels/${roomId}/messages`,
          { headers: { Cookie: cookie(alice) } },
        );
        assert.equal(textResult.status, 404);
      },
    );
    await t.test("deleting a voice room ejects participants and prevents rejoining", async () => {
      const left = next(a, "voice:room-left");
      const response = await fetch(`${url}/communities/${communityId}/channels/${roomId}`, {
        method: "DELETE", headers: { Cookie: cookie(alice) },
      });
      assert.equal(response.status, 200);
      assert.equal((await left).reason, "channelDeleted");
      assert.ok((await send(a, "voice:room-join", payload)).error);
    });
    await t.test("deleting a community closes active voice rooms", async () => {
      const current = await Community.findById(communityId);
      const music = String(current.channels.find(channel => channel.name === "music")._id);
      assert.ok((await send(a, "voice:room-join", { communityId, channelId: music })).sessionId);
      const left = next(a, "voice:room-left");
      const response = await fetch(`${url}/communities/${communityId}`, { method: "DELETE", headers: { Cookie: cookie(alice) } });
      assert.equal(response.status, 200);
      assert.equal((await left).reason, "channelDeleted");
      assert.ok((await send(a, "voice:room-join", { communityId, channelId: music })).error);
    });
  } finally {
    sockets.forEach((socket) => socket.disconnect());
    await new Promise((resolve) => io.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
