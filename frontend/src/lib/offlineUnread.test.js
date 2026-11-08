import "fake-indexeddb/auto";
import { test } from "node:test";
import assert from "node:assert/strict";
import { candidatePosition, newer, readPosition, rememberPosition, readChannelPosition, rememberChannelPosition } from "./offlineUnread.js";

test("read positions are account scoped and advance atomically across concurrent writes", async () => {
  const alice = "aaaaaaaaaaaaaaaaaaaaaaaa", bob = "bbbbbbbbbbbbbbbbbbbbbbbb", carol = "cccccccccccccccccccccccc";
  const older = { createdAt: "2026-01-01T00:00:00.000Z", messageId: "000000000000000000000001" };
  const latest = { createdAt: older.createdAt, messageId: "000000000000000000000002" };
  await Promise.all([rememberPosition(alice, bob, latest), rememberPosition(alice, bob, older)]);
  assert.deepEqual(await readPosition(alice, bob), latest);
  assert.equal(await readPosition(carol, bob), null);
  assert.equal(newer(latest, older), true);
  assert.equal(newer(older, latest), false);
  await assert.rejects(rememberPosition(alice, bob, { ...latest, messageId: "invalid" }));
  assert.throws(() => candidatePosition({ ...latest, createdAt: "invalid" }));
  await rememberChannelPosition(alice, bob, carol, older);
  assert.deepEqual(await readChannelPosition(alice, bob, carol), older);
  assert.equal(await readChannelPosition(carol, bob, carol), null);
  assert.deepEqual(await readPosition(alice, bob), latest);
});
