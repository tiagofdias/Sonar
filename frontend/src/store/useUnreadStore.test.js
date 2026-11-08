import { test } from "node:test";
import assert from "node:assert/strict";
import { useUnreadStore } from "./useUnreadStore.js";
test("unread indicators are local to unseen direct conversations", () => {
  const state = useUnreadStore.getState();
  state.reset();
  state.setActivePeer("a");
  state.markDirect("a");
  state.markDirect("b");
  state.markDirect("b");
  assert.deepEqual(useUnreadStore.getState().direct, { b: true });
  state.setActivePeer("b");
  assert.deepEqual(useUnreadStore.getState().direct, {});
  state.setActivePeer(null);
  state.markDirect("b");
  assert.deepEqual(useUnreadStore.getState().direct, { b: true });
  state.reset();
  assert.deepEqual(useUnreadStore.getState().direct, {});
});

test("channel indicators clear on opening or community removal", () => {
  const state = useUnreadStore.getState();
  state.reset();
  state.setActiveChannel("a:one");
  state.markChannel("a:one");
  state.markChannel("a:two");
  state.markChannel("b:one");
  assert.deepEqual(useUnreadStore.getState().channels, { "a:two": true, "b:one": true });
  state.setActiveChannel("a:two");
  state.clearCommunity("b");
  assert.deepEqual(useUnreadStore.getState().channels, {});
  state.reset();
});
