import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import attachmentRoutes, { MAX_FILE_BYTES } from "./attachment.route.js";
import Attachment from "../models/attachment.model.js";
import User from "../models/user.model.js";
import { befriend } from "../lib/friends.js";
import Community from "../models/community.model.js";

test("encrypted files are limited, access-controlled and owner-deletable", async () => {
  const database = await MongoMemoryServer.create({ instance: { ip: "127.0.0.1" } });
  let server;
  try {
    process.env.JWT_SECRET ||= "x".repeat(40);
    await mongoose.connect(database.getUri("sonar-attachment-tests"));
    const make = (name) => User.create({ fullName: name, email: `${name}@example.invalid`, password: "hashed-password" });
    const [alice, bob, eve, dan] = await Promise.all(["alice", "bob", "eve", "dan"].map(make));
    await befriend(alice._id, bob._id);
    const community = await Community.create({ name: "Crew", ownerId: alice._id, members: [alice._id, dan._id], channels: [{ name: "general" }] });
    const app = express();
    app.use(cookieParser());
    app.use("/files", attachmentRoutes);
    server = await new Promise((resolve) => { const s = app.listen(0, "127.0.0.1", () => resolve(s)); });
    const url = `http://127.0.0.1:${server.address().port}/files`;
    const call = (user, path, method = "GET", body) => fetch(url + path, {
      method,
      headers: { Cookie: `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}`, "Content-Type": "application/octet-stream" },
      body,
    });
    const bytes = Buffer.alloc(100, 7);
    const sent = await call(alice, `/?to=${bob._id}`, "POST", bytes);
    assert.equal(sent.status, 201);
    const { id } = await sent.json();
    assert.deepEqual(Buffer.from(await (await call(bob, `/${id}`)).arrayBuffer()), bytes);
    assert.equal((await call(eve, `/${id}`)).status, 404, "strangers cannot fetch it");
    assert.equal((await call(alice, "/", "POST", bytes)).status, 400, "a destination is required");
    assert.equal((await call(alice, `/?to=${bob._id}`, "POST", Buffer.alloc(MAX_FILE_BYTES + 17))).status, 413);
    assert.equal((await call(eve, `/?community=${community._id}`, "POST", bytes)).status, 404, "only members can upload");
    const shared = await call(alice, `/?community=${community._id}`, "POST", bytes);
    const sharedId = (await shared.json()).id;
    assert.equal((await call(dan, `/${sharedId}`)).status, 200);
    assert.equal((await call(eve, `/${sharedId}`)).status, 404);
    await call(bob, `/${id}`, "DELETE");
    assert.ok(await Attachment.findById(id), "only the owner can delete it");
    await call(alice, `/${id}`, "DELETE");
    assert.equal(await Attachment.findById(id), null);
    assert.ok((await Attachment.findById(sharedId)).expiresAt > new Date());
  } finally {
    server?.closeAllConnections?.();
    await new Promise((resolve) => (server ? server.close(resolve) : resolve()));
    await mongoose.disconnect();
    await database.stop();
  }
});
