import { test } from "node:test";
import assert from "node:assert/strict";
import { completeMention, mentionQuery, mentionsIn, mentionsMe } from "./mentions.js";
import { decodeMessage, encodeMessage } from "./messageContent.js";

const members = [{ _id: "a", fullName: "Ana Costa" }, { _id: "b", fullName: "Bo" }];

test("mentions are found, completed and limited to allowed senders", () => {
  assert.deepEqual(mentionsIn("hi @ana costa and @everyone", members), ["a", "everyone"]);
  assert.deepEqual(mentionsIn("email me at x@bo", members), ["b"]);
  assert.equal(mentionQuery("hello @an"), "an");
  assert.equal(mentionQuery("mail@an"), null);
  assert.equal(completeMention("hello @an", "Ana Costa"), "hello @Ana Costa ");
  assert.equal(mentionsMe({ mentions: ["everyone"] }, "z", false), false);
  assert.equal(mentionsMe({ mentions: ["everyone"] }, "z", true), true);
  assert.equal(mentionsMe({ mentions: ["z"] }, "z", false), true);
});

test("mentions travel inside the message and bad lists are ignored", () => {
  const id = "0123456789abcdef01234567";
  assert.deepEqual(decodeMessage(encodeMessage("hi", null, [id, "everyone"])), { text: "hi", mentions: [id, "everyone"] });
  assert.equal(encodeMessage("plain", null, []), "plain");
  assert.deepEqual(decodeMessage('sonar-reply-v1:{"text":"hi","mentions":["bad id"]}'), { text: "hi" });
});
