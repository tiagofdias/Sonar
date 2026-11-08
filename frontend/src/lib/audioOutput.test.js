import { test } from "node:test";
import assert from "node:assert/strict";
import { applyAudioOutput } from "./audioOutput.js";
test("audio output routes to the chosen sink and applies volume", async () => {
  let selected;
  const player = {
    volume: 1,
    setSinkId: async (id) => {
      selected = id;
    },
  };
  await applyAudioOutput(player, {
    outputDeviceId: "headphones",
    outputVolume: 0.37,
  });
  assert.equal(selected, "headphones");
  assert.equal(player.volume, 0.37);
  await applyAudioOutput(player, { outputDeviceId: "", outputVolume: 0 });
  assert.equal(selected, "");
  assert.equal(player.volume, 0);
});
test("unavailable output fails without silently switching to another device", async () => {
  const error = new Error("Unavailable speakers");
  let attempts = 0;
  const player = {
    setSinkId: async () => {
      attempts++;
      throw error;
    },
  };
  await assert.rejects(
    applyAudioOutput(player, { outputDeviceId: "missing", outputVolume: 0.5 }),
    /Unavailable speakers/,
  );
  assert.equal(attempts, 1);
  await assert.rejects(
    applyAudioOutput({}, { outputDeviceId: "headphones", outputVolume: 0.5 }),
    /system speakers/,
  );
  const fallback = {};
  await applyAudioOutput(fallback, { outputDeviceId: "", outputVolume: 0.5 });
  assert.equal(fallback.volume, 0.5);
});
