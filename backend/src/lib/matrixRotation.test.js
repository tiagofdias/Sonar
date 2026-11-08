import { test } from "node:test";
import assert from "node:assert/strict";
import "fake-indexeddb/auto";
import { OlmMachine, UserId, DeviceId, RoomId, EncryptionSettings, CollectStrategy } from "@matrix-org/matrix-sdk-crypto-wasm";
import { ensureMatrixRoomRevision } from "../../../frontend/src/lib/matrixRoomRevision.js";

const locks = new Map();
Object.defineProperty(globalThis.navigator, "locks", { configurable: true, value: { async request(name, _options, callback) {
  const previous = locks.get(name) || Promise.resolve();
  let release; const next = new Promise(resolve => { release = resolve; });
  locks.set(name, next); await previous;
  try { return await callback(); } finally { if (locks.get(name) === next) locks.delete(name); release(); }
} } });

test("browser rotates SDK room session on revision changes and blocks rollback", async () => {
  const user = new UserId("@0123456789abcdef01234567:sonar.invalid"), device = new DeviceId("DEVICE"), room = new RoomId("!rotation:sonar.invalid");
  const machine = await OlmMachine.initialize(user, device);
  const sdk = { RoomId }, store = `sonar-matrix-v2-${"a".repeat(32)}`;
  const settings = new EncryptionSettings(); settings.sharingStrategy = CollectStrategy.allDevices();
  try {
    assert.equal(await ensureMatrixRoomRevision(sdk, machine, store, room.toString(), 0), true);
    await machine.shareRoomKey(room, [], settings);
    const first = JSON.parse(await machine.encryptRoomEvent(room, "m.room.message", JSON.stringify({ msgtype: "m.text", body: "one" })));
    assert.equal(await ensureMatrixRoomRevision(sdk, machine, store, room.toString(), 0), false);
    const second = JSON.parse(await machine.encryptRoomEvent(room, "m.room.message", JSON.stringify({ msgtype: "m.text", body: "two" })));
    assert.equal(first.session_id, second.session_id);
    const results = await Promise.all([ensureMatrixRoomRevision(sdk, machine, store, room.toString(), 1), ensureMatrixRoomRevision(sdk, machine, store, room.toString(), 1)]);
    assert.deepEqual(results.sort(), [false, true]);
    await assert.rejects(ensureMatrixRoomRevision(sdk, machine, store, room.toString(), 0), /Sending is blocked/);
    await machine.shareRoomKey(room, [], settings);
    const third = JSON.parse(await machine.encryptRoomEvent(room, "m.room.message", JSON.stringify({ msgtype: "m.text", body: "three" })));
    assert.notEqual(first.session_id, third.session_id);
  } finally { settings.free(); machine.close(); room.free(); user.free(); device.free(); }
});
