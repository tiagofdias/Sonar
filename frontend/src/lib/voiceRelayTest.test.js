import test from "node:test";
import assert from "node:assert/strict";
import { testVoiceRelay } from "./voiceRelayTest.js";

class FakePeer {
  constructor(config) { this.config = config; FakePeer.last = this; }
  createDataChannel() {}
  async createOffer() { return {}; }
  async setLocalDescription() {}
  close() { this.closed = true; }
}
test("relay test requires a relay candidate and closes the connection", async () => {
  const probe = testVoiceRelay([{ urls: "turn:example.invalid" }], { Peer: FakePeer });
  const pc = FakePeer.last;
  assert.equal(pc.config.iceTransportPolicy, "relay");
  pc.onicecandidate({ candidate: { type: "relay" } });
  assert.equal(await probe.promise, "available");
  assert.equal(pc.closed, true);
});
test("relay test reports failure and can be cancelled", async () => {
  let probe = testVoiceRelay([], { Peer: FakePeer });
  FakePeer.last.onicecandidate({ candidate: null });
  assert.equal(await probe.promise, "unavailable");
  probe = testVoiceRelay([], { Peer: FakePeer });
  const pc = FakePeer.last;
  probe.cancel();
  assert.equal(await probe.promise, "cancelled");
  assert.equal(pc.closed, true);
});
