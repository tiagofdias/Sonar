import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { watchSocketSession } from "./socketSession.js";

function fakeSocket() {
  const socket = new EventEmitter();
  socket.disconnect = () => { socket.disconnected = true; socket.emit("disconnect"); };
  return socket;
}

test("idle sockets close on revoked sessions and verification failures", async () => {
  for (const verify of [async () => false, async () => { throw new Error("database offline"); }]) {
    const socket = fakeSocket();
    let events = 0;
    socket.on("sessionRevoked", () => events++);
    const watcher = watchSocketSession(socket, verify, 60000);
    await watcher.check();
    await watcher.check();
    assert.equal(events, 1);
    assert.equal(socket.disconnected, true);
  }
});

test("socket checks do not overlap and stop after disconnect", async () => {
  const socket = fakeSocket();
  let calls = 0, finish;
  const watcher = watchSocketSession(socket, () => {
    calls++;
    return new Promise(resolve => { finish = resolve; });
  }, 60000);
  const pending = watcher.check();
  await watcher.check();
  assert.equal(calls, 1);
  socket.disconnect();
  finish(false);
  await pending;
  await watcher.check();
  assert.equal(calls, 1);
});

test("a stuck verification eventually disconnects the socket", async () => {
  const socket = fakeSocket();
  const watcher = watchSocketSession(socket, () => new Promise(() => {}), 60000, 10);
  // Keep the test process alive while the unreferenced production deadline fires.
  const keepAlive = setTimeout(() => {}, 100);
  await watcher.check();
  clearTimeout(keepAlive);
  assert.equal(socket.disconnected, true);
});
