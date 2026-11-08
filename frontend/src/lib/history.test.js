import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeHistory, withoutExpired } from "./history.js";
import { useDraftStore } from "../store/useDraftStore.js";
import { refreshedContact } from "./contacts.js";
test("contact refresh uses the newest key and removes unavailable contacts", () => {
  const old = { _id: "alice", fullName: "Alice", encryptionKey: "old-key" };
  const latest = { ...old, encryptionKey: "new-key" };
  assert.equal(refreshedContact(old, [latest]), latest);
  assert.equal(refreshedContact(old, []), null);
  assert.equal(refreshedContact(null, [latest]), null);
});
test("history merges overlapping pages and realtime edits in stable timestamp order", () => {
  const time = "2026-11-04T12:00:00.000Z";
  const old = { _id: "b", createdAt: time, text: "old", pinned: false };
  const changed = { ...old, pinned: true };
  const page = [{ _id: "a", createdAt: time }, old];
  const stream = [changed, { _id: "c", createdAt: time }];
  const result = mergeHistory(page, stream);
  assert.deepEqual(
    result.map((m) => m._id),
    ["a", "b", "c"],
  );
  assert.equal(result[1].pinned, true);
  assert.equal(result.length, 3);
});
test("drafts are isolated by conversation and removed on membership loss or signout", () => {
  const store = useDraftStore.getState();
  store.update("dm:bob", "Private draft");
  store.update("channel:crew:general", "Community draft");
  store.update("channel:other:general", "Keep this draft");
  store.clearPrefix("channel:crew:");
  assert.equal(
    useDraftStore.getState().drafts["channel:crew:general"],
    undefined,
  );
  assert.equal(useDraftStore.getState().drafts["dm:bob"], "Private draft");
  assert.equal(
    useDraftStore.getState().drafts["channel:other:general"],
    "Keep this draft",
  );
  store.reset();
  assert.deepEqual(useDraftStore.getState().drafts, {});
});
test("expired messages and reactions leave active history while pins remain", () => {
  const now = Date.parse("2030-01-01T00:00:00.000Z");
  const messages = [
    { _id: "expired", expiresAt: new Date(now).toISOString() },
    { _id: "reaction", reaction: {}, expiresAt: new Date(now - 1).toISOString() },
    { _id: "live", expiresAt: new Date(now + 1).toISOString() },
    { _id: "pinned", pinned: true, expiresAt: null },
    { _id: "invalid", expiresAt: "invalid" },
  ];
  assert.deepEqual(withoutExpired(messages, now).map(message => message._id), ["live", "pinned"]);
  assert.equal(messages.length, 5);
});
