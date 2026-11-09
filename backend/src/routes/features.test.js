import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import friendsRoutes from "./friends.route.js";
import messageRoutes from "./message.route.js";
import preferencesRoutes from "./preferences.route.js";
import communityRoutes, { can, channelName } from "./community.route.js";
import authRoutes from "./auth.route.js";
import User from "../models/user.model.js";
import Message from "../models/message.model.js";
import ChannelMessage from "../models/channelMessage.model.js";
import Friendship from "../models/friendship.model.js";
import { befriend, areFriends, seedFriendsFromMessages } from "../lib/friends.js";
import { allowedEndpoint } from "../lib/push.js";

const envelope = { version: 1, iv: "AAAAAAAAAAAAAAAA", ciphertext: "AAAAAAAAAAAAAAAAAAAAAAAA" };

test("channel names and moderator permissions", () => {
  assert.equal(channelName("  Weekend   Plans "), "Weekend Plans");
  assert.equal(channelName("../bad"), "");
  assert.equal(channelName("x".repeat(41)), "");
  const community = { ownerId: "o", moderators: ["m", "n"], moderatorLimits: [{ userId: "m", denied: ["delete"] }], kind: "server" };
  assert.equal(can(community, "o", "delete"), true);
  assert.equal(can(community, "m", "delete"), false);
  assert.equal(can(community, "m", "pin"), true);
  assert.equal(can(community, "n", "delete"), true, "existing moderators keep everything");
  assert.equal(can(community, "x", "pin"), false);
  assert.equal(can({ ...community, kind: "group" }, "x", "pin"), true);
});

test("alert endpoints are limited to browser push services", () => {
  assert.equal(allowedEndpoint("https://fcm.googleapis.com/fcm/send/a"), true);
  assert.equal(allowedEndpoint("https://updates.push.services.mozilla.com/a"), true);
  assert.equal(allowedEndpoint("http://fcm.googleapis.com/a"), false);
  assert.equal(allowedEndpoint("https://169.254.169.254/latest"), false);
  assert.equal(allowedEndpoint("https://fcm.googleapis.com.evil.test/a"), false);
});

test("edits, blocks, disappearing messages, groups and preferences", async (t) => {
  process.env.JWT_SECRET ||= "test-secret-test-secret-test-secret-1";
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
  let server;
  try {
    await mongoose.connect(database.getUri("sonar-features-tests"));
    const make = (name) => User.create({ fullName: `${name} Test`, username: name, email: `${name}@example.invalid`, password: "hashed", encryptionKey: "key" });
    const [ana, bo, cy, di] = await Promise.all(["ana", "bo", "cy", "di"].map(make));
    await Promise.all([befriend(ana._id, bo._id), befriend(ana._id, cy._id)]);
    const app = express();
    app.use(cookieParser());
    app.use(express.json());
    app.use("/auth", authRoutes);
    app.use("/friends", friendsRoutes);
    app.use("/messages", messageRoutes);
    app.use("/preferences", preferencesRoutes);
    app.use("/communities", communityRoutes);
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

    await t.test("only the sender can edit a direct message", async () => {
      const sent = (await call(ana, `/messages/send/${bo._id}`, "POST", envelope)).body;
      const changed = { ...envelope, ciphertext: "BBBBBBBBBBBBBBBBBBBBBBBB" };
      assert.equal((await call(bo, `/messages/${sent._id}`, "PATCH", changed)).status, 404);
      assert.equal((await call(ana, `/messages/${sent._id}`, "PATCH", { text: "plain" })).status, 400);
      const edited = await call(ana, `/messages/${sent._id}`, "PATCH", changed);
      assert.equal(edited.status, 200);
      assert.equal(edited.body.ciphertext, changed.ciphertext);
      assert.ok(edited.body.editedAt);
    });

    await t.test("disappearing messages shorten expiry and stop pins", async () => {
      assert.equal((await call(ana, `/friends/${bo._id}/disappear`, "PUT", { seconds: 5 })).status, 400);
      assert.equal((await call(ana, `/friends/${di._id}/disappear`, "PUT", { seconds: 3600 })).status, 404);
      assert.equal((await call(bo, `/friends/${ana._id}/disappear`, "PUT", { seconds: 3600 })).status, 200);
      const sent = (await call(ana, `/messages/send/${bo._id}`, "POST", envelope)).body;
      assert.ok(new Date(sent.expiresAt).getTime() < Date.now() + 3700000);
      assert.equal((await call(ana, `/messages/${sent._id}/pin`, "PATCH", { pinned: true })).status, 409);
      assert.equal((await call(ana, "/messages/users")).body.find((p) => p._id === String(bo._id)).disappearAfter, 3600);
      await call(bo, `/friends/${ana._id}/disappear`, "PUT", { seconds: 0 });
    });

    await t.test("a request can carry a note only its recipient gets", async () => {
      assert.equal((await call(di, "/friends/request", "POST", { username: "cy", note: { version: 1, iv: "bad", ciphertext: "x" } })).status, 400);
      assert.equal((await call(di, "/friends/request", "POST", { username: "cy", note: envelope })).status, 201);
      assert.equal((await call(cy, "/friends")).body.incoming[0].note.ciphertext, envelope.ciphertext);
      assert.equal((await call(cy, "/friends")).body.incoming[0].encryptionKey, "key");
      assert.equal((await call(di, "/friends")).body.outgoing[0].note, undefined);
      await call(cy, `/friends/${di._id}`, "DELETE");
    });

    await t.test("a block cuts contact, is not revealed and survives a restart", async () => {
      await call(ana, `/messages/send/${cy._id}`, "POST", envelope);
      assert.equal((await call(ana, `/preferences/block/${cy._id}`, "POST")).status, 200);
      assert.equal(await areFriends(ana._id, cy._id), false);
      assert.equal((await call(cy, `/messages/send/${ana._id}`, "POST", envelope)).status, 403);
      assert.equal((await call(cy, "/friends/request", "POST", { username: "ana" })).status, 201, "looks like it worked");
      assert.equal((await call(ana, "/friends")).body.incoming.length, 0);
      assert.equal((await call(ana, "/friends/request", "POST", { username: "cy" })).status, 409);
      await seedFriendsFromMessages();
      assert.equal(await areFriends(ana._id, cy._id), false);
      assert.deepEqual((await call(ana, "/preferences")).body.blocked.map((p) => p.username), ["cy"]);
      assert.equal((await call(ana, `/preferences/block/${cy._id}`, "DELETE")).status, 200);
      assert.equal(await Friendship.exists({ status: "blocked" }), null);
      await Message.deleteMany({ receiverId: cy._id });
      await befriend(ana._id, cy._id);
    });

    await t.test("preferences are validated and animals come from the list", async () => {
      assert.equal((await call(ana, "/preferences", "PUT", { mutedCommunities: ["nope"] })).status, 400);
      assert.equal((await call(ana, "/preferences", "PUT", { folders: [{ name: "", people: [] }] })).status, 400);
      const saved = await call(ana, "/preferences", "PUT", { pinnedPeople: [String(bo._id)], folders: [{ name: "Family", people: [String(bo._id)] }], typingHidden: true });
      assert.equal(saved.body.typingHidden, true);
      assert.equal(saved.body.folders[0].name, "Family");
      assert.equal((await call(ana, "/auth/update-profile", "PUT", { fullName: "Ana", animal: "dragon" })).status, 400);
      assert.equal((await call(ana, "/auth/update-profile", "PUT", { fullName: "Ana", animal: "orca" })).body.animal, "orca");
    });

    await t.test("groups are friends only and hold no invite link", async () => {
      assert.equal((await call(ana, "/communities/group", "POST", { memberIds: [String(bo._id)] })).status, 400);
      assert.equal((await call(ana, "/communities/group", "POST", { memberIds: [String(bo._id), String(di._id)] })).status, 403);
      const group = await call(ana, "/communities/group", "POST", { memberIds: [String(bo._id), String(cy._id)] });
      assert.equal(group.status, 201);
      assert.equal(group.body.kind, "group");
      assert.equal(group.body.name, "Ana, bo, cy");
      assert.equal((await call(ana, `/communities/${group.body._id}/invite`, "POST")).status, 409);
    });

    await t.test("channels: edits stay with original readers, permissions and the log", async () => {
      const created = (await call(ana, "/communities", "POST", { name: "Crew" })).body;
      const root = `/communities/${created._id}`;
      const token = (await call(ana, `${root}/invite`, "POST")).body.token;
      await call(bo, "/communities/join", "POST", { token });
      const read = async () => (await call(ana, root)).body;
      const channel = (await read()).channels[0]._id;
      let info = await read();
      const sent = (await call(ana, `${root}/channels/${channel}/messages`, "POST", { revision: info.revision, envelopes: [ana, bo].map((u) => ({ ...envelope, recipientId: String(u._id) })) })).body;
      await call(cy, "/communities/join", "POST", { token });
      info = await read();
      const edit = await call(ana, `${root}/channels/${channel}/messages/${sent._id}`, "PATCH", { revision: info.revision, envelopes: [ana, bo, cy].map((u) => ({ ...envelope, ciphertext: "CCCCCCCCCCCCCCCCCCCCCCCC", recipientId: String(u._id) })) });
      assert.equal(edit.status, 200);
      assert.ok(edit.body.editedAt);
      assert.equal((await ChannelMessage.findById(sent._id)).envelopes.length, 2, "a later joiner gains nothing");
      assert.equal((await call(bo, `${root}/channels/${channel}/messages/${sent._id}`, "DELETE")).status, 404);
      await call(ana, `${root}/members/${bo._id}/moderator`, "PUT", { moderator: true });
      await call(ana, `${root}/members/${bo._id}/permissions`, "PUT", { denied: ["remove"] });
      assert.equal((await call(bo, `${root}/members/${cy._id}`, "DELETE")).status, 403);
      assert.equal((await call(bo, `${root}/channels/${channel}/messages/${sent._id}`, "DELETE")).status, 200);
      assert.equal((await call(cy, `${root}/activity`)).status, 403);
      await call(cy, `${root}/leave`, "POST");
      const log = (await call(bo, `${root}/activity`)).body;
      assert.deepEqual(log.slice(0, 2).map((row) => row.action), ["left", "joined"]);
      assert.equal((await read()).activity, undefined, "only the log route shows it");
    });
  } finally {
    server?.close();
    await mongoose.disconnect();
    await database.stop();
  }
});
