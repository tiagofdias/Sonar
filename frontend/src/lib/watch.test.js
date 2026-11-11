import test from "node:test";
import assert from "node:assert/strict";
import { clockOffset, drifted, expectedPosition, formatTime, shouldPlay, waitingNames } from "./watch.js";

const state = (extra) => ({ current: 0, playing: true, position: 10, serverNow: 1_000_000, hold: null, viewers: [], ...extra });

test("the shared position advances only while playing and nobody is being waited for", () => {
  const offset = clockOffset(state(), 999_000); // this computer's clock is one second behind the server
  assert.equal(offset, 1000);
  assert.equal(expectedPosition(state(), offset, 1_001_500), 12.5);
  assert.equal(expectedPosition(state({ playing: false }), offset, 1_001_500), 10);
  assert.equal(expectedPosition(state({ hold: { waitingFor: ["b"] } }), offset, 1_001_500), 10);
  assert.equal(expectedPosition(state({ current: -1 }), offset), 0);
});

test("during a hold only the people being waited for keep playing", () => {
  const held = state({ hold: { waitingFor: ["bo"] } });
  assert.equal(shouldPlay(held, "bo"), true, "their ad or buffering has to finish");
  assert.equal(shouldPlay(held, "ana"), false);
  assert.equal(shouldPlay(state(), "ana"), true);
  assert.equal(shouldPlay(state({ playing: false }), "ana"), false);
});

test("players correct themselves after drifting more than a second", () => {
  assert.equal(drifted(10.4, 10), false);
  assert.equal(drifted(11.2, 10), true);
  assert.equal(drifted(8.9, 10), true);
});

test("times and waiting names read naturally", () => {
  assert.equal(formatTime(65), "1:05");
  assert.equal(formatTime(3725), "1:02:05");
  const viewers = ["Ana", "Bo", "Cy", "Di"].map((name) => ({ userId: name.toLowerCase(), name }));
  assert.equal(waitingNames({ hold: { waitingFor: ["bo"] }, viewers }), "Bo");
  assert.equal(waitingNames({ hold: { waitingFor: ["bo", "cy"] }, viewers }), "Bo and Cy");
  assert.equal(waitingNames({ hold: { waitingFor: ["ana", "bo", "cy", "di"] }, viewers }), "Ana, Bo and 2 more");
});
