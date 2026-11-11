import test from "node:test";
import assert from "node:assert/strict";
import { roomStatus } from "./roomStatus.js";

test("room status describes the audio links in plain words", () => {
  assert.equal(roomStatus({ joining: true, others: 1, connected: 0 }), "Connecting…");
  assert.equal(roomStatus({ others: 0, connected: 0 }), "Waiting for others to join");
  assert.equal(roomStatus({ others: 1, connected: 0 }), "Connecting…");
  assert.equal(roomStatus({ others: 1, connected: 1 }), "Connected");
  assert.equal(roomStatus({ others: 3, connected: 3 }), "Connected to everyone");
  assert.equal(roomStatus({ others: 3, connected: 1 }), "Connected to 1 of 3");
  assert.equal(roomStatus({ others: 1, connected: 0, failed: 1 }), "Couldn't connect · leave and rejoin");
  assert.equal(roomStatus({ others: 3, connected: 2, failed: 1 }), "Couldn't reach 1 of 3 · leave and rejoin");
});
