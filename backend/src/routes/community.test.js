import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import communityRoutes, {
  validEnvelopes,
  forRecipient,
} from "./community.route.js";
import User from "../models/user.model.js";
import Community from "../models/community.model.js";
import ChannelMessage from "../models/channelMessage.model.js";

test("community authorization, invitation, encrypted history and retention", async (t) => {
  const database = await MongoMemoryReplSet.create({
    replSet: { count: 1, ip: "127.0.0.1" },
  });
  let server;
  try {
    // Explicit isolated URI: never uses MONGODB_URI from a developer's environment.
    await mongoose.connect(database.getUri("sonar-community-tests"));
    const users = await User.create(
      ["Owner", "Friend", "Newcomer"].map((fullName, i) => ({
        fullName,
        email: `test-${i}@example.invalid`,
        password: "unused-test-hash",
        encryptionKey: "test-public-key",
      })),
    );
    const app = express();
    app.use(express.json({ limit: "800kb" }));
    app.use(cookieParser());
    app.use("/communities", communityRoutes);
    server = await new Promise((resolve) => {
      const instance = app.listen(0, "127.0.0.1", () => resolve(instance));
    });
    const url = `http://127.0.0.1:${server.address().port}/communities`;
    async function request(user, path = "", method = "GET", body) {
      const headers = { "Content-Type": "application/json" };
      if (user)
        headers.Cookie = `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "5m" })}`;
      const res = await fetch(url + path, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: res.status, data: await res.json() };
    }
    const [owner, friend, newcomer] = users;
    let community, channel, token, sent;
    const envelope = (recipient) => ({
      recipientId: String(recipient._id),
      version: 1,
      iv: "AAAAAAAAAAAAAAAA",
      ciphertext: "AAAAAAAAAAAAAAAAAAAAAAAA",
    });
    await t.test(
      "requires login and creates owner membership and default channel",
      async () => {
        assert.equal((await request(null)).status, 401);
        const created = await request(owner, "", "POST", {
          name: "Private crew",
        });
        assert.equal(created.status, 201);
        community = created.data;
        channel = community.channels[0];
        assert.equal(channel.name, "general");
        assert.deepEqual(community.members, [String(owner._id)]);
        assert.equal((await request(friend, `/${community._id}`)).status, 404);
        assert.deepEqual((await request(friend)).data, []);
      },
    );
    await t.test(
      "invites are hashed, owner-only, revocable, and join is idempotent",
      async () => {
        assert.equal(
          (await request(friend, `/${community._id}/invite`, "POST")).status,
          404,
        );
        const first = await request(owner, `/${community._id}/invite`, "POST");
        const second = await request(owner, `/${community._id}/invite`, "POST");
        token = second.data.token;
        assert.equal(token.length, 48);
        const stored = await Community.findById(community._id).select(
          "+inviteHash",
        );
        assert.notEqual(stored.inviteHash, token);
        assert.equal(
          (await request(friend, "/join", "POST", { token: first.data.token }))
            .status,
          404,
        );
        const before = await request(friend, "/invite-preview", "POST", { token });
        assert.equal(before.data.available, true);
        assert.equal(before.data.name, community.name);
        assert.equal(before.data.memberCount, 1);
        assert.equal(before.data.member, false);
        assert.equal(before.data.id, undefined);
        assert.equal((await request(friend, "/invite-preview", "POST", { token: "0".repeat(48) })).data.available, false);
        assert.equal((await request(friend, "/invite-preview", "POST", { token: "nope" })).data.available, false);
        assert.equal(
          (await request(friend, "/join", "POST", { token })).status,
          200,
        );
        const after = await request(friend, "/invite-preview", "POST", { token });
        assert.equal(after.data.member, true);
        assert.equal(after.data.memberCount, 2);
        assert.equal(after.data.id, String(community._id));
        assert.equal(
          (await request(friend, "/join", "POST", { token })).status,
          200,
        );
        const detail = await request(owner, `/${community._id}`);
        community = detail.data;
        assert.equal(community.members.length, 2);
        assert.equal(community.revision, 1);
        assert.equal(community.inviteHash, undefined);
        assert.equal(community.memberDetails[0].password, undefined);
      },
    );
    await t.test("only owners can rename communities and channels", async () => {
      const root = `/${community._id}`;
      assert.equal((await request(friend, root, "PATCH", { name: "No" })).status, 403);
      assert.equal((await request(owner, root, "PATCH", { name: "   " })).status, 400);
      assert.equal((await request(owner, root, "PATCH", { name: "New crew" })).status, 200);
      assert.equal((await request(friend, root)).data.name, "New crew");
      const path = `${root}/channels/${channel._id}`;
      assert.equal((await request(friend, path, "PATCH", { name: "no" })).status, 403);
      assert.equal((await request(owner, path, "PATCH", { name: "../bad" })).status, 400);
      assert.equal((await request(owner, path, "PATCH", { name: "General Chat" })).status, 200);
      assert.equal((await request(friend, root)).data.channels[0].name, "General Chat");
      assert.equal((await request(owner, path, "PATCH", { name: "general" })).status, 200);
      assert.equal((await request(owner, `${root}/channels/000000000000000000000000`, "PATCH", { name: "x" })).status, 404);
    });
    await t.test(
      "only owner creates channels; invalid and duplicate names fail",
      async () => {
        assert.equal(
          (
            await request(friend, `/${community._id}/channels`, "POST", {
              name: "secret",
            })
          ).status,
          403,
        );
        assert.equal(
          (
            await request(owner, `/${community._id}/channels`, "POST", {
              name: "../bad",
            })
          ).status,
          400,
        );
        assert.equal(
          (
            await request(owner, `/${community._id}/channels`, "POST", {
              name: "general",
            })
          ).status,
          409,
        );
        assert.equal(
          (
            await request(owner, `/${community._id}/channels`, "POST", {
              name: "Weekend plans",
            })
          ).status,
          201,
        );
      },
    );
    await t.test("channel rename rejects a name used by another channel", async () => {
      assert.equal((await request(owner, `/${community._id}/channels/${channel._id}`, "PATCH", { name: "weekend plans" })).status, 409);
      assert.equal((await request(owner, `/${community._id}`)).data.channels[0].name, "general");
    });
    const path = () => `/${community._id}/channels/${channel._id}/messages`;
    await t.test(
      "rejects plaintext, missing recipients, outsiders and stale membership",
      async () => {
        assert.equal(
          (
            await request(owner, path(), "POST", {
              text: "private",
              revision: 1,
            })
          ).status,
          400,
        );
        assert.equal(
          (
            await request(owner, path(), "POST", {
              revision: 0,
              envelopes: [envelope(owner), envelope(friend)],
            })
          ).status,
          409,
        );
        assert.equal(
          (
            await request(owner, path(), "POST", {
              revision: 1,
              envelopes: [envelope(owner)],
            })
          ).status,
          400,
        );
        assert.equal((await request(newcomer, path())).status, 404);
        const result = await request(owner, path(), "POST", {
          revision: 1,
          envelopes: [envelope(owner), envelope(friend)],
        });
        assert.equal(result.status, 201);
        sent = result.data;
        assert.equal(sent.envelopes, undefined);
        assert.equal(sent.receiverId, String(owner._id));
        const history = await request(friend, path());
        assert.equal(history.data.length, 1);
        assert.equal(history.data[0].receiverId, String(friend._id));
        assert.equal(history.data[0].envelopes, undefined);
      },
    );
    await t.test(
      "late joiners cannot read or pin earlier ciphertext",
      async () => {
        assert.equal(
          (await request(newcomer, "/join", "POST", { token })).status,
          200,
        );
        assert.deepEqual((await request(newcomer, path())).data, []);
        assert.equal(
          (
            await request(newcomer, `${path()}/${sent._id}/pin`, "PATCH", {
              pinned: true,
            })
          ).status,
          404,
        );
        assert.equal(
          (
            await request(owner, path(), "POST", {
              revision: 1,
              envelopes: [envelope(owner), envelope(friend)],
            })
          ).status,
          409,
        );
      },
    );
    await t.test(
      "pin preserves history; expiry hides records and prevents revival",
      async () => {
        const pinned = await request(
          owner,
          `${path()}/${sent._id}/pin`,
          "PATCH",
          { pinned: true },
        );
        assert.equal(pinned.status, 200);
        assert.equal(pinned.data.expiresAt, null);
        assert.equal((await request(owner, path())).data.length, 1);
        const unpinned = await request(
          owner,
          `${path()}/${sent._id}/pin`,
          "PATCH",
          { pinned: false },
        );
        assert.ok(
          new Date(unpinned.data.expiresAt) >
            new Date(Date.now() + 6 * 86400000),
        );
        await ChannelMessage.updateOne(
          { _id: sent._id },
          { expiresAt: new Date(Date.now() - 1000) },
        );
        assert.deepEqual((await request(owner, path())).data, []);
        assert.equal(
          (
            await request(owner, `${path()}/${sent._id}/pin`, "PATCH", {
              pinned: true,
            })
          ).status,
          404,
        );
      },
    );
    await t.test("expired invite fails", async () => {
      await Community.updateOne(
        { _id: community._id },
        { inviteExpiresAt: new Date(0) },
      );
      assert.equal(
        (await request(friend, "/join", "POST", { token })).status,
        404,
      );
    });
    await t.test(
      "validates exact recipient set and strips other envelopes",
      () => {
        assert.equal(
          validEnvelopes(
            [envelope(owner), envelope(owner)],
            [owner._id, friend._id],
          ),
          false,
        );
        assert.equal(
          validEnvelopes(
            [envelope(owner), envelope(newcomer)],
            [owner._id, friend._id],
          ),
          false,
        );
        assert.equal(
          validEnvelopes(
            [envelope(owner), envelope(friend)],
            [owner._id, friend._id],
          ),
          true,
        );
        assert.equal(
          forRecipient({ envelopes: [envelope(owner)] }, friend._id),
          null,
        );
      },
    );
    await t.test("only a current member may delete their own community message", async () => {
      const created = await request(owner, path(), "POST", { revision: 2, envelopes: [envelope(owner), envelope(friend), envelope(newcomer)] });
      assert.equal(created.status, 201);
      const endpoint = `${path()}/${created.data._id}`;
      assert.equal((await request(friend, endpoint, "DELETE")).status, 404);
      const outsider = await User.create({ fullName: "Outsider", email: "outsider-delete@example.invalid", password: "unused-test-hash" });
      assert.equal((await request(outsider, endpoint, "DELETE")).status, 404);
      assert.equal((await request(owner, endpoint, "DELETE")).status, 200);
      assert.equal(await ChannelMessage.exists({ _id: created.data._id }), null);
      assert.equal((await request(owner, endpoint, "DELETE")).status, 404);
    });
    await t.test(
      "member removal revokes invites, blocks stale sends, and preserves authors for remaining readers",
      async () => {
        const created = (
          await request(owner, "", "POST", { name: "Managed crew" })
        ).data;
        const root = `/${created._id}`;
        const messages = `${root}/channels/${created.channels[0]._id}/messages`;
        const invite = (await request(owner, `${root}/invite`, "POST")).data
          .token;
        await request(friend, "/join", "POST", { token: invite });
        await request(newcomer, "/join", "POST", { token: invite });
        assert.equal(
          (
            await request(friend, messages, "POST", {
              revision: 2,
              envelopes: [
                envelope(owner),
                envelope(friend),
                envelope(newcomer),
              ],
            })
          ).status,
          201,
        );
        assert.equal(
          (await request(friend, `${root}/members/${newcomer._id}`, "DELETE"))
            .status,
          403,
        );
        assert.equal(
          (await request(owner, `${root}/members/${owner._id}`, "DELETE"))
            .status,
          400,
        );
        assert.equal(
          (await request(owner, `${root}/members/${friend._id}`, "DELETE"))
            .status,
          200,
        );
        assert.equal((await request(friend, root)).status, 404);
        assert.equal((await request(friend, messages)).status, 404);
        assert.equal(
          (await request(friend, "/join", "POST", { token: invite })).status,
          404,
        );
        assert.equal(
          (
            await request(owner, messages, "POST", {
              revision: 2,
              envelopes: [
                envelope(owner),
                envelope(friend),
                envelope(newcomer),
              ],
            })
          ).status,
          409,
        );
        const fresh = await request(owner, messages, "POST", {
          revision: 3,
          envelopes: [envelope(owner), envelope(newcomer)],
        });
        assert.equal(fresh.status, 201);
        const stored = await ChannelMessage.findById(fresh.data._id);
        assert.ok(
          stored.envelopes.every(
            (e) => String(e.recipientId) !== String(friend._id),
          ),
        );
        const detail = (await request(owner, root)).data;
        assert.ok(
          !detail.memberDetails.some((m) => m._id === String(friend._id)),
        );
        assert.ok(
          detail.historyAuthors.some((m) => m._id === String(friend._id)),
        );
        assert.equal(
          (await request(owner, `${root}/leave`, "POST")).status,
          409,
        );
        assert.equal(
          (
            await request(newcomer, `${root}/transfer`, "POST", {
              memberId: String(owner._id),
            })
          ).status,
          403,
        );
        assert.equal(
          (
            await request(owner, `${root}/transfer`, "POST", {
              memberId: String(friend._id),
            })
          ).status,
          409,
        );
        assert.equal(
          (
            await request(owner, `${root}/transfer`, "POST", {
              memberId: String(newcomer._id),
            })
          ).status,
          200,
        );
        assert.equal(
          (await request(owner, `${root}/invite`, "POST")).status,
          403,
        );
        assert.equal(
          (
            await request(owner, `${root}/channels`, "POST", {
              name: "forbidden",
            })
          ).status,
          403,
        );
        assert.equal(
          (await request(owner, `${root}/members/${newcomer._id}`, "DELETE"))
            .status,
          403,
        );
        assert.equal(
          (await request(owner, `${root}/leave`, "POST")).status,
          200,
        );
        assert.equal((await request(owner, messages)).status, 404);
        assert.equal(
          (await request(newcomer, `${root}/leave`, "POST")).status,
          409,
        );
      },
    );
    await t.test(
      "a membership change after validation aborts the transactional send",
      async () => {
        const created = (
          await request(owner, "", "POST", { name: "Race check" })
        ).data;
        const root = `/${created._id}`;
        const invite = (await request(owner, `${root}/invite`, "POST")).data
          .token;
        await request(friend, "/join", "POST", { token: invite });
        const original = Community.findOneAndUpdate;
        let injected = false;
        Community.findOneAndUpdate = function (filter, update, options) {
          if (!injected && update.$inc?.activityVersion) {
            injected = true;
            return (async () => {
              await original.call(
                this,
                { _id: created._id },
                { $pull: { members: friend._id }, $inc: { revision: 1 } },
              );
              return original.call(this, filter, update, options);
            })();
          }
          return original.call(this, filter, update, options);
        };
        try {
          const result = await request(
            owner,
            `${root}/channels/${created.channels[0]._id}/messages`,
            "POST",
            { revision: 1, envelopes: [envelope(owner), envelope(friend)] },
          );
          assert.equal(result.status, 409);
          assert.equal(
            await ChannelMessage.countDocuments({ communityId: created._id }),
            0,
          );
          assert.equal(injected, true);
        } finally {
          Community.findOneAndUpdate = original;
        }
      },
    );
    await t.test("owners can delete channels and their pinned history while retaining one text channel", async () => {
      const created = (await request(owner, "", "POST", { name: "Delete test" })).data;
      const root = `/${created._id}`;
      const general = created.channels[0]._id;
      assert.equal((await request(owner, `${root}/channels/${general}`, "DELETE")).status, 409);
      const extra = (await request(owner, `${root}/channels`, "POST", { name: "extra" })).data.channels.at(-1)._id;
      const message = (await request(owner, `${root}/channels/${extra}/messages`, "POST", { revision: 0, envelopes: [envelope(owner)] })).data;
      assert.equal((await request(owner, `${root}/channels/${extra}/messages/${message._id}/pin`, "PATCH", { pinned: true })).status, 200);
      assert.equal((await request(friend, `${root}/channels/${extra}`, "DELETE")).status, 404);
      assert.equal((await request(owner, `${root}/channels/${extra}`, "DELETE")).status, 200);
      assert.equal(await ChannelMessage.exists({ _id: message._id }), null);
      assert.equal((await request(owner, `${root}/channels/${extra}/messages`)).status, 404);
      assert.equal((await request(owner, `${root}`)).data.channels.length, 1);
    });
    await t.test("community deletion is owner-only and deletes stored pinned messages", async () => {
      const created = (await request(owner, "", "POST", { name: "Delete community test" })).data;
      const root = `/${created._id}`;
      const invite = (await request(owner, `${root}/invite`, "POST")).data.token;
      assert.equal((await request(friend, "/join", "POST", { token: invite })).status, 200);
      const messages = `${root}/channels/${created.channels[0]._id}/messages`;
      const sent = (await request(owner, messages, "POST", { revision: 1, envelopes: [envelope(owner), envelope(friend)] })).data;
      assert.equal((await request(owner, `${messages}/${sent._id}/pin`, "PATCH", { pinned: true })).status, 200);
      assert.equal((await request(friend, root, "DELETE")).status, 403);
      assert.ok(await Community.exists({ _id: created._id }));
      assert.equal((await request(owner, root, "DELETE")).status, 200);
      assert.equal(await Community.exists({ _id: created._id }), null);
      assert.equal(await ChannelMessage.countDocuments({ communityId: created._id }), 0);
      assert.equal((await request(friend, root)).status, 404);
      assert.equal((await request(friend, "/join", "POST", { token: invite })).status, 404);
    });
    await t.test("moderator permissions protect the owner and other moderators", async () => {
      const regular = await User.create({ fullName: "Regular", email: "moderator-regular@example.invalid", password: "unused", encryptionKey: "key" });
      const regularTwo = await User.create({ fullName: "Regular Two", email: "moderator-regular-two@example.invalid", password: "unused", encryptionKey: "key" });
      const created = await Community.create({ name: "Moderation test", ownerId: owner._id, members: [owner._id, friend._id, newcomer._id, regular._id, regularTwo._id], channels: [{ name: "general" }] });
      const root = `/${created._id}`;
      const role = member => `${root}/members/${member._id}/moderator`;
      const remove = member => `${root}/members/${member._id}`;
      assert.equal((await request(friend, role(friend), "PUT", { moderator: true })).status, 403);
      assert.equal((await request(owner, role(friend), "PUT", { moderator: true })).status, 200);
      assert.equal((await request(owner, role(newcomer), "PUT", { moderator: true })).status, 200);
      assert.equal((await request(regular, remove(regularTwo), "DELETE")).status, 403);
      assert.equal((await request(friend, remove(owner), "DELETE")).status, 409);
      assert.equal((await request(friend, remove(newcomer), "DELETE")).status, 409);
      assert.equal((await request(friend, remove(regular), "DELETE")).status, 200);
      assert.equal((await request(friend, `${root}/invite`, "POST")).status, 403);
      assert.equal((await request(owner, role(friend), "PUT", { moderator: false })).status, 200);
      assert.equal((await request(friend, remove(regularTwo), "DELETE")).status, 403);
      assert.equal((await request(newcomer, `${root}/leave`, "POST")).status, 200);
      assert.equal((await Community.findById(created._id)).moderators.length, 0);
      assert.equal((await request(owner, role(friend), "PUT", { moderator: true })).status, 200);
      assert.equal((await request(owner, `${root}/transfer`, "POST", { memberId: String(friend._id) })).status, 200);
      const final = await Community.findById(created._id);
      assert.equal(String(final.ownerId), String(friend._id));
      assert.equal(final.moderators.length, 0);
      assert.equal((await request(owner, role(regularTwo), "PUT", { moderator: true })).status, 403);
    });
    await t.test("bans block new invites until an owner unbans and issues a fresh invite", async () => {
      const regular = await User.create({ fullName: "Ban target", email: "ban-target@example.invalid", password: "unused", encryptionKey: "test-key" });
      const created = await Community.create({ name: "Ban test", ownerId: owner._id, members: [owner._id, friend._id, newcomer._id, regular._id], moderators: [friend._id, newcomer._id], channels: [{ name: "general" }] });
      const root = `/${created._id}`;
      const ban = member => `${root}/members/${member._id}/ban`;
      assert.equal((await request(regular, ban(friend), "POST")).status, 403);
      assert.equal((await request(friend, ban(owner), "POST")).status, 409);
      assert.equal((await request(friend, ban(newcomer), "POST")).status, 409);
      const oldInvite = (await request(owner, `${root}/invite`, "POST")).data.token;
      assert.equal((await request(friend, ban(regular), "POST")).status, 200);
      assert.equal((await request(regular, "/join", "POST", { token: oldInvite })).status, 404);
      const currentInvite = (await request(owner, `${root}/invite`, "POST")).data.token;
      assert.equal((await request(regular, "/join", "POST", { token: currentInvite })).status, 404);
      assert.equal((await request(friend, `${root}/bans`)).status, 403);
      assert.equal((await request(friend, `${root}/bans/${regular._id}`, "DELETE")).status, 403);
      assert.equal((await request(owner, `${root}/bans`)).data[0]._id, String(regular._id));
      assert.equal((await request(friend, root)).data.banned, undefined);
      assert.equal((await request(owner, `${root}/bans/${regular._id}`, "DELETE")).status, 200);
      assert.equal((await request(regular, "/join", "POST", { token: currentInvite })).status, 404);
      assert.equal((await request(regular, root)).status, 404);
      const freshInvite = (await request(owner, `${root}/invite`, "POST")).data.token;
      assert.equal((await request(regular, "/join", "POST", { token: freshInvite })).status, 200);
      assert.equal((await request(owner, `${root}/bans`)).data.length, 0);
    });
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
