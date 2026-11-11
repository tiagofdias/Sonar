import { test } from "node:test";
import assert from "node:assert/strict";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import { io as client } from "socket.io-client";
process.env.JWT_SECRET ||= "test-secret-test-secret-test-secret-1";
const { server, presenceAudienceChanged } = await import("./socket.js");
const { default: User } = await import("../models/user.model.js");
const { default: Community } = await import("../models/community.model.js");
const { befriend } = await import("./friends.js");

test("who is online is only told to friends and people in the same community", async () => {
  const database = await MongoMemoryServer.create();
  const sockets = [];
  try {
    await mongoose.connect(database.getUri("sonar-presence-tests"));
    const [ana, bo, cy, di] = await User.create(["ana", "bo", "cy", "di"].map((name) => ({ fullName: name, username: name, email: `${name}@example.invalid`, password: "unused-test-hash" })));
    await befriend(ana._id, bo._id);
    await Community.create({ name: "Reef", ownerId: cy._id, members: [cy._id, di._id], channels: [{ name: "general" }] });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    const seen = new Map();
    const connect = async (user) => {
      const socket = client(url, { transports: ["websocket"], reconnection: false, autoConnect: false, extraHeaders: { Cookie: `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}` } });
      sockets.push(socket);
      socket.on("getOnlineUsers", (ids) => seen.set(String(user._id), [...ids].sort()));
      await new Promise((resolve, reject) => { socket.once("connect", resolve); socket.once("connect_error", reject); socket.connect(); });
      return socket;
    };
    for (const user of [ana, bo, cy, di]) await connect(user);
    const settle = () => new Promise((resolve) => setTimeout(resolve, 400));
    const ids = (...users) => users.map((user) => String(user._id)).sort();
    await settle();
    assert.deepEqual(seen.get(String(ana._id)), ids(ana, bo), "friends see each other");
    assert.deepEqual(seen.get(String(bo._id)), ids(ana, bo));
    assert.deepEqual(seen.get(String(cy._id)), ids(cy, di), "community members see each other");
    assert.deepEqual(seen.get(String(di._id)), ids(cy, di), "strangers stay hidden");
    await befriend(ana._id, cy._id);
    presenceAudienceChanged(ana._id, cy._id);
    await settle();
    assert.deepEqual(seen.get(String(ana._id)), ids(ana, bo, cy), "a new friend appears at once");
    assert.deepEqual(seen.get(String(cy._id)), ids(ana, cy, di));
    assert.deepEqual(seen.get(String(bo._id)), ids(ana, bo), "a friend of a friend stays hidden");
  } finally {
    for (const socket of sockets) socket.disconnect();
    await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
