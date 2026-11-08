import test from "node:test";
import assert from "node:assert/strict";
import { refreshVoiceConfiguration } from "./voiceConfigRefresh.js";

test("refreshes before expiry and retries failures", async () => {
  let task, delay, calls = 0, applied;
  const stop = refreshVoiceConfiguration(
    { expiresAt: new Date(3600000).toISOString() },
    async () => { if (++calls === 1) throw new Error("offline"); return { expiresAt: new Date(7200000).toISOString() }; },
    value => { applied = value; },
    { now: () => 0, schedule: (fn, ms) => { task = fn; delay = ms; return 1; }, cancel: () => { task = null; } },
  );
  assert.equal(delay, 3300000);
  await task();
  assert.equal(delay, 30000);
  await task();
  assert.equal(applied.expiresAt, new Date(7200000).toISOString());
  stop();
  assert.equal(task, null);
});

test("ignores a response received after cleanup", async () => {
  let task, resolve, applied = false;
  const stop = refreshVoiceConfiguration(
    { expiresAt: new Date(301000).toISOString() },
    () => new Promise(r => { resolve = r; }),
    () => { applied = true; },
    { now: () => 0, schedule: fn => { task = fn; }, cancel: () => {} },
  );
  const pending = task();
  stop();
  resolve({ expiresAt: new Date(3600000).toISOString() });
  await pending;
  assert.equal(applied, false);
});
