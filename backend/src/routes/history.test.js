import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import messageRoutes from "./message.route.js";
import communityRoutes from "./community.route.js";
import User from "../models/user.model.js";
import { befriend } from "../lib/friends.js";
import Message from "../models/message.model.js";
import Community from "../models/community.model.js";
import ChannelMessage from "../models/channelMessage.model.js";
test("cursor history preserves ordering, visibility, expiry, and independent pin access", async (t) => {
  const database = await MongoMemoryServer.create();
  let server;
  try {
    await mongoose.connect(database.getUri("sonar-history-tests"));
    const [alice, bob, outsider] = await User.create(
      ["Alice", "Bob", "Outsider"].map((fullName, i) => ({
        fullName,
        email: `history-${i}@example.invalid`,
        password: "unused-test-hash",
      })),
    );
    await befriend(alice._id, bob._id);
    const community = await Community.create({
      name: "History crew",
      ownerId: alice._id,
      members: [alice._id, bob._id],
      channels: [{ name: "general" }],
    });
    const channelId = community.channels[0]._id;
    const sameTime = new Date(Date.now() - 60000);
    const envelope = {
      ciphertext: "AAAAAAAAAAAAAAAAAAAAAAAA",
      iv: "AAAAAAAAAAAAAAAA",
      version: 1,
    };
    const direct = Array.from({ length: 123 }, (_, i) => ({
      _id: new mongoose.Types.ObjectId(),
      senderId: alice._id,
      receiverId: bob._id,
      ...envelope,
      createdAt: sameTime,
      expiresAt: new Date(Date.now() + 86400000),
      pinned: i === 0,
    }));
    const channel = direct.map((m, i) => ({
      _id: new mongoose.Types.ObjectId(),
      senderId: alice._id,
      communityId: community._id,
      channelId,
      createdAt: sameTime,
      expiresAt: m.expiresAt,
      pinned: i === 0,
      envelopes: [
        { recipientId: alice._id, ...envelope },
        { recipientId: bob._id, ...envelope },
      ],
    }));
    await Message.insertMany(direct);
    await ChannelMessage.insertMany(channel);
    const app = express();
    app.use(express.json());
    app.use(cookieParser());
    app.use("/messages", messageRoutes);
    app.use("/communities", communityRoutes);
    server = await new Promise((resolve) => {
      const instance = app.listen(0, "127.0.0.1", () => resolve(instance));
    });
    const url = `http://127.0.0.1:${server.address().port}`;
    async function request(path, user = alice) {
      const r = await fetch(url + path, {
        headers: {
          Cookie: `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET)}`,
        },
      });
      return { status: r.status, body: await r.json() };
    }
    await t.test("offline unread candidates contain only live received message metadata", async () => {
      const expired = await Message.create({ senderId: outsider._id, receiverId: bob._id, ...envelope, expiresAt: new Date(0) });
      const result = await request("/messages/unread-candidates", bob);
      assert.equal(result.status, 200);
      assert.deepEqual(result.body, { items: [{ senderId: String(alice._id), messageId: String(direct.at(-1)._id), createdAt: sameTime.toISOString() }], next: null });
      assert.deepEqual((await request("/messages/unread-candidates", outsider)).body.items, []);
      assert.deepEqual((await request("/messages/unread-candidates", alice)).body.items, []);
      await Message.updateOne({ _id: expired._id }, { $set: { pinned: true, expiresAt: null } });
      const withPinned = await request("/messages/unread-candidates", bob);
      assert.ok(withPinned.body.items.some(item => item.senderId === String(outsider._id) && item.messageId === String(expired._id)));
      await Message.deleteOne({ _id: expired._id });
    });
    await t.test("unread candidate pages do not truncate contacts", async () => {
      const synthetic = Array.from({ length: 205 }, () => ({ senderId: new mongoose.Types.ObjectId(), receiverId: outsider._id, ...envelope }));
      await Message.insertMany(synthetic);
      let after, ids = [];
      do {
        const page = await request(`/messages/unread-candidates${after ? `?after=${after}` : ""}`, outsider);
        assert.equal(page.status, 200);
        assert.ok(page.body.items.length <= 100);
        ids.push(...page.body.items.map(item => item.senderId));
        after = page.body.next;
      } while (after);
      assert.equal(ids.length, 205);
      assert.equal(new Set(ids).size, 205);
      assert.equal((await request("/messages/unread-candidates?after=invalid", outsider)).status, 400);
      await Message.deleteMany({ receiverId: outsider._id });
    });
    await t.test("channel unread metadata respects membership, envelopes, and retention", async () => {
      const path = `/communities/${community._id}/unread-candidates`;
      const hidden = await ChannelMessage.create({ communityId: community._id, channelId, senderId: alice._id, envelopes: [{ recipientId: alice._id, ...envelope }] });
      const expired = await ChannelMessage.create({ communityId: community._id, channelId, senderId: alice._id, envelopes: [{ recipientId: bob._id, ...envelope }], expiresAt: new Date(0) });
      const bobResult = await request(path, bob);
      assert.deepEqual(bobResult.body, [{ channelId: String(channelId), messageId: String(channel.at(-1)._id), createdAt: sameTime.toISOString() }]);
      assert.deepEqual((await request(path, alice)).body, []);
      assert.equal((await request(path, outsider)).status, 404);
      await ChannelMessage.deleteMany({ _id: { $in: [hidden._id, expired._id] } });
    });
    const paths = [
      `/messages/${bob._id}`,
      `/communities/${community._id}/channels/${channelId}/messages`,
    ];
    for (const [index, path] of paths.entries()) {
      const source = index === 0 ? direct : channel;
      await t.test(
        `${index === 0 ? "direct" : "channel"} pagination has no gaps at identical timestamps`,
        async () => {
          let cursor,
            ids = [];
          do {
            const result = await request(
              `${path}?page=true${cursor ? `&before=${cursor}` : ""}`,
            );
            assert.equal(result.status, 200);
            assert.ok(result.body.messages.length <= 50);
            if (!cursor)
              assert.equal(
                result.body.messages.at(-1)._id,
                String(source.at(-1)._id),
              );
            if (index === 1)
              assert.ok(
                result.body.messages.every(
                  (m) => !m.envelopes && m.receiverId === String(alice._id),
                ),
              );
            ids.push(...result.body.messages.map((m) => m._id));
            cursor = result.body.nextCursor;
            assert.equal(result.body.hasMore, Boolean(cursor));
          } while (cursor);
          assert.equal(ids.length, 123);
          assert.equal(new Set(ids).size, 123);
          assert.deepEqual(
            [...ids].sort(),
            source.map((m) => String(m._id)).sort(),
          );
        },
      );
      await t.test(
        `${index === 0 ? "direct" : "channel"} pins outside the recent page remain reachable`,
        async () => {
          const result = await request(`${path}?page=true&pinned=true`);
          assert.equal(result.body.messages.length, 1);
          assert.equal(result.body.messages[0]._id, String(source[0]._id));
          assert.equal(result.body.hasMore, false);
          for (const query of [
            "before=not-a-cursor",
            "limit=101",
            "limit=0",
            "pinned=invalid",
          ])
            assert.equal(
              (await request(`${path}?page=true&${query}`)).status,
              400,
            );
        },
      );
    }
    await t.test(
      "expiry is applied on every page and outsiders cannot traverse history",
      async () => {
        await Message.updateOne(
          { _id: direct[60]._id },
          { expiresAt: new Date(0) },
        );
        await ChannelMessage.updateOne(
          { _id: channel[60]._id },
          { expiresAt: new Date(0) },
        );
        assert.equal(
          (await request(`/messages/${bob._id}?page=true`, outsider)).status,
          403,
        );
        assert.equal(
          (await request(paths[1] + "?page=true", outsider)).status,
          404,
        );
        const second = (await request(paths[0] + "?page=true&limit=100")).body;
        assert.ok(
          !second.messages.some((m) => m._id === String(direct[60]._id)),
        );
        const next = (await request(paths[1] + "?page=true&limit=100")).body;
        assert.ok(
          !next.messages.some((m) => m._id === String(channel[60]._id)),
        );
      },
    );
    await t.test("pin changes preserve the original deadline unless a pin is actually removed", async () => {
      for (const [model, target, path] of [
        [Message, direct[1]._id, `/messages/${direct[1]._id}/pin`],
        [ChannelMessage, channel[1]._id, `/communities/${community._id}/channels/${community.channels[0]._id}/messages/${channel[1]._id}/pin`],
      ]) {
        const update = async pinned => fetch(url + path, {
          method: "PATCH",
          headers: { "Content-Type": "application/json", Cookie: `jwt=${jwt.sign({ userId: String(alice._id) }, process.env.JWT_SECRET)}` },
          body: JSON.stringify({ pinned }),
        });
        const original = (await model.findById(target)).expiresAt.getTime();
        assert.equal((await update(false)).status, 200);
        assert.equal((await model.findById(target)).expiresAt.getTime(), original);
        assert.equal((await update(true)).status, 200);
        assert.equal((await model.findById(target)).expiresAt, null);
        assert.equal((await update(false)).status, 200);
        const deadline = (await model.findById(target)).expiresAt.getTime();
        assert.ok(deadline > original);
        assert.equal((await update(false)).status, 200);
        assert.equal((await model.findById(target)).expiresAt.getTime(), deadline);
      }
    });
    await t.test("only the sender can delete a direct message, including pinned messages", async () => {
      const target = direct[0]._id;
      const remove = async user => {
        const response = await fetch(`${url}/messages/${target}`, {
          method: "DELETE",
          headers: { Cookie: `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET)}` },
        });
        return response.status;
      };
      assert.equal(await remove(bob), 404);
      assert.equal(await remove(outsider), 404);
      assert.ok(await Message.exists({ _id: target }));
      assert.equal(await remove(alice), 200);
      assert.equal(await Message.exists({ _id: target }), null);
      assert.equal(await remove(alice), 404);
      assert.equal((await request(`${paths[0]}?page=true&pinned=true`)).body.messages.length, 0);
    });
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
