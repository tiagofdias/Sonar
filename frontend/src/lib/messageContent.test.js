import { test } from "node:test";
import assert from "node:assert/strict";
import { encodeMessage, decodeMessage } from "./messageContent.js";
test("reply target is encoded with encrypted message content and decoded", () => {
  const id = "0123456789abcdef01234567";
  assert.deepEqual(decodeMessage(encodeMessage("Hi", id)), { text: "Hi", replyTo: id });
  assert.throws(() => encodeMessage("Hi", "invalid"));
});
test("legacy and prefix-like messages remain readable", () => {
  for (const value of ["plain text", "sonar-reply-v1:hello", 'sonar-reply-v1:{"text":"safe"}'])
    assert.deepEqual(decodeMessage(encodeMessage(value)), { text: value });
  assert.deepEqual(decodeMessage("sonar-reply-v1:not-json"), { text: "sonar-reply-v1:not-json" });
});
