import { test } from "node:test";
import assert from "node:assert/strict";
import { encodeMessage, encodeReaction, decodeMessage, REACTION_EMOJI } from "./messageContent.js";
import { reactionSummary } from "./reactions.js";
const target = "0123456789abcdef01234567", emoji = REACTION_EMOJI[0];
test("reactions encode an authenticated message event and preserve ordinary text", () => {
  const reaction = { target, emoji, action: "add" };
  assert.deepEqual(decodeMessage(encodeReaction(reaction)), { text: "", reaction });
  assert.throws(() => encodeReaction({ ...reaction, target: "invalid" }));
  assert.throws(() => encodeReaction({ ...reaction, emoji: "invalid" }));
  const text = `sonar-reaction-v1:${JSON.stringify(reaction)}`;
  assert.deepEqual(decodeMessage(encodeMessage(text)), { text });
});
test("reactions reduce per sender and emoji, and expire with their events", () => {
  const event = (senderId, action, expiry = 10000) => ({ senderId, reaction: { target, emoji, action }, expiresAt: new Date(expiry).toISOString() });
  assert.deepEqual(reactionSummary([event("a", "add"), event("b", "add"), event("a", "remove")], 1000), { [target]: { [emoji]: ["b"] } });
  assert.deepEqual(reactionSummary([event("a", "add", 1000)], 1000), {});
});
