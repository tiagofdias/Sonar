import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import friendsRoutes from "./friends.route.js";
import messageRoutes from "./message.route.js";
import User from "../models/user.model.js";
import Message from "../models/message.model.js";
import Friendship from "../models/friendship.model.js";
import { seedFriendsFromMessages, areFriends } from "../lib/friends.js";

test("direct contact needs an accepted friend request", async (t) => {
  process.env.JWT_SECRET ||= "test-secret-test-secret-test-secret-1";
  const database = await MongoMemoryServer.create();
  let server;
  try {
    await mongoose.connect(database.getUri("sonar-friends-tests"));
    const make = (name) => User.create({ fullName: name, username: name, email: `${name}@example.invalid`, password: "hashed", encryptionKey: "key" });
    const [ana, bo, cy, di] = await Promise.all(["ana", "bo", "cy", "di"].map(make));
    const app = express();
    app.use(cookieParser());
    app.use(express.json());
    app.use("/friends", friendsRoutes);
    app.use("/messages", messageRoutes);
    server = await new Promise((resolve) => { const s = app.listen(0, "127.0.0.1", () => resolve(s)); });
    const url = `http://127.0.0.1:${server.address().port}`;
    const call = async (user, path, method = "GET", body) => {
      const response = await fetch(url + path, {
        method,
        headers: { Cookie: `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}`, "Content-Type": "application/json" },
        body: body ? JSON.stringify(body) : undefined,
      });
      return { status: response.status, body: await response.json() };
    };
    const envelope = { version: 1, iv: "AAAAAAAAAAAAAAAA", ciphertext: "AAAAAAAAAAAAAAAAAAAAAAAA" };

    await t.test("a new account sees nobody and cannot reach strangers", async () => {
      assert.deepEqual((await call(ana, "/messages/users")).body, []);
      assert.equal((await call(ana, `/messages/send/${bo._id}`, "POST", envelope)).status, 403);
      assert.equal((await call(ana, `/messages/${bo._id}`)).status, 403);
    });
    await t.test("bad usernames are refused", async () => {
      assert.equal((await call(ana, "/friends/request", "POST", { username: "nobody" })).status, 404);
      assert.equal((await call(ana, "/friends/request", "POST", { username: "ana" })).status, 400);
      assert.equal((await call(ana, "/friends/request", "POST", { username: { $ne: "" } })).status, 400);
    });
    await t.test("a request stays pending until the other person accepts", async () => {
      assert.equal((await call(ana, "/friends/request", "POST", { username: "@BO" })).status, 201);
      assert.equal((await call(ana, "/friends/request", "POST", { username: "bo" })).status, 409);
      assert.deepEqual((await call(ana, "/friends")).body.outgoing.map((p) => p.username), ["bo"]);
      const incoming = (await call(bo, "/friends")).body.incoming;
      assert.deepEqual(incoming.map((p) => p.username), ["ana"]);
      assert.equal(incoming[0].encryptionKey, undefined);
      assert.equal((await call(ana, `/messages/send/${bo._id}`, "POST", envelope)).status, 403);
      assert.equal((await call(ana, `/friends/${bo._id}/accept`, "POST")).status, 404, "the sender cannot accept for them");
      assert.equal((await call(bo, `/friends/${ana._id}/accept`, "POST")).status, 200);
      assert.deepEqual((await call(ana, "/messages/users")).body.map((p) => p.username), ["bo"]);
      assert.deepEqual((await call(bo, "/messages/users")).body.map((p) => p.username), ["ana"]);
      assert.equal((await call(ana, `/messages/send/${bo._id}`, "POST", envelope)).status, 201);
      assert.equal((await call(ana, "/friends/request", "POST", { username: "bo" })).status, 409);
    });
    await t.test("declining removes the request and leaves no access", async () => {
      await call(cy, "/friends/request", "POST", { username: "ana" });
      assert.equal((await call(ana, `/friends/${cy._id}`, "DELETE")).status, 200);
      assert.deepEqual((await call(cy, "/friends")).body.outgoing, []);
      assert.equal((await call(cy, `/messages/send/${ana._id}`, "POST", envelope)).status, 403);
      assert.equal((await call(ana, `/friends/${bo._id}`, "DELETE")).status, 404, "declining cannot remove an accepted friend");
    });
    await t.test("asking someone who already asked you makes you friends", async () => {
      await call(cy, "/friends/request", "POST", { username: "di" });
      assert.equal((await call(di, "/friends/request", "POST", { username: "cy" })).body.status, "accepted");
      assert.equal(await areFriends(cy._id, di._id), true);
    });
    await t.test("people already talking are kept, and seeding can repeat", async () => {
      const eli = await make("eli");
      await Message.create({ senderId: eli._id, receiverId: ana._id, ...envelope, expiresAt: new Date(Date.now() + 86400000) });
      await seedFriendsFromMessages();
      await seedFriendsFromMessages();
      assert.equal(await areFriends(eli._id, ana._id), true);
      assert.equal(await Friendship.countDocuments({ users: eli._id }), 1);
      assert.equal(await areFriends(eli._id, bo._id), false);
    });
  } finally {
    await new Promise((resolve) => (server ? server.close(resolve) : resolve()));
    await mongoose.disconnect();
    await database.stop();
  }
});
