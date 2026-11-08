import "fake-indexeddb/auto";
import { test } from "node:test";
import assert from "node:assert/strict";
import { processVerificationInbox, isInterruptedVerificationReceipt, clearVerificationReceipts } from "./matrixVerificationInbox.js";

test("interrupted SDK processing fails closed rather than replaying an event", async () => {
  const descriptor = { storeName: `sonar-matrix-v2-${"c".repeat(32)}` };
  const row = { id: "a".repeat(24), event: { type: "m.room.encrypted", content: { ciphertext: "synthetic" } } };
  let calls = 0;
  const sdk = { DeviceLists: class { free() {} }, ProcessedToDeviceEventType: { Decrypted: 0 } };
  const machine = { async receiveSyncChangesMsc4186() { calls++; return [{ type: 0, free() {} }]; } };
  await assert.rejects(processVerificationInbox(machine, descriptor, [row], sdk, async () => { throw new Error("simulated crash after SDK call"); }), /simulated crash/);
  assert.equal(await isInterruptedVerificationReceipt(descriptor, row), true);
  await assert.rejects(processVerificationInbox(machine, descriptor, [row], sdk, async () => {}), /processing was interrupted/);
  assert.equal(calls, 1);
  assert.equal(await isInterruptedVerificationReceipt(descriptor, { ...row, event: { ...row.event, content: { ciphertext: "different" } } }), false);
  await clearVerificationReceipts(descriptor, [row.id]);
  assert.deepEqual(await processVerificationInbox(machine, descriptor, [row], sdk, async () => {}), [row.id]);
  assert.deepEqual(await processVerificationInbox(machine, descriptor, [row], sdk, async () => {}), [row.id]);
  assert.equal(calls, 2);
  await clearVerificationReceipts(descriptor, [row.id]);
});
