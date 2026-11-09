import { test } from "node:test";
import assert from "node:assert/strict";
import Message, { WEEK } from "./message.model.js";
import Friendship from "./friendship.model.js";
import {
  sendMessage,
  getMessages,
  pinMessage,
} from "../controllers/message.controller.js";
const senderId = "012345678901234567890123",
  receiverId = "012345678901234567890124";
const response = () => ({
  code: 200,
  status(code) {
    this.code = code;
    return this;
  },
  json(body) {
    this.body = body;
    return this;
  },
});
test("message schema stores envelopes and sets a one-week TTL", () => {
  const start = Date.now();
  const m = new Message({
    senderId,
    receiverId,
    ciphertext: "encrypted",
    iv: "iv",
    text: "must not persist",
  });
  assert.equal(m.text, undefined);
  assert.ok(m.expiresAt.getTime() >= start + WEEK);
  assert.ok(
    Message.schema
      .indexes()
      .some(
        ([keys, options]) =>
          keys.expiresAt === 1 && options.expireAfterSeconds === 0,
      ),
  );
});
test("plaintext send requests are rejected before touching storage", async () => {
  const res = response();
  await sendMessage(
    {
      body: { text: "secret" },
      params: { id: receiverId },
      user: { _id: senderId },
    },
    res,
  );
  assert.equal(res.code, 400);
});
test("history excludes expired records and is scoped to both participants", async (t) => {
  t.mock.method(Friendship, "exists", async () => true);
  const original = Message.find;
  let filter;
  Message.find = (value) => {
    filter = value;
    return { sort: () => ({ limit: async () => [] }) };
  };
  try {
    const res = response();
    await getMessages(
      { params: { id: receiverId }, user: { _id: senderId } },
      res,
    );
    assert.equal(res.code, 200);
    assert.equal(filter.$and[0].$or[0].pinned, true);
    assert.ok(filter.$and[0].$or[1].expiresAt.$gt instanceof Date);
    assert.deepEqual(filter.$and[1].$or, [
      { senderId, receiverId },
      { senderId: receiverId, receiverId: senderId },
    ]);
  } finally {
    Message.find = original;
  }
});
test("pin authorization requires participation and unexpired history", async (t) => {
  const original = Message.findOneAndUpdate;
  t.mock.method(Message, "findById", () => ({ select: () => ({ lean: async () => null }) }));
  let filter, update;
  Message.findOneAndUpdate = async (f, u) => {
    filter = f;
    update = u;
    return null;
  };
  try {
    const res = response();
    await pinMessage(
      {
        params: { id: receiverId },
        user: { _id: senderId },
        body: { pinned: true },
      },
      res,
    );
    assert.equal(res.code, 404);
    assert.equal(update[0].$set.expiresAt, null);
    assert.deepEqual(filter.$and[1].$or, [
      { senderId },
      { receiverId: senderId },
    ]);
    await pinMessage(
      {
        params: { id: receiverId },
        user: { _id: senderId },
        body: { pinned: false },
      },
      response(),
    );
    assert.ok(update[0].$set.expiresAt.$cond[1].getTime() >= Date.now() + WEEK - 1000);
  } finally {
    Message.findOneAndUpdate = original;
  }
});
