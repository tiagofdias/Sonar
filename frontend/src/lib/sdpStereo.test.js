import test from "node:test";
import assert from "node:assert/strict";
import { preferStereo } from "./sdpStereo.js";

test("a music or TV answer asks for stereo Opus, once", () => {
  const sdp = "v=0\r\na=rtpmap:111 opus/48000/2\r\na=fmtp:111 minptime=10;useinbandfec=1\r\na=rtpmap:0 PCMU/8000\r\n";
  const stereo = preferStereo(sdp);
  assert.match(stereo, /a=fmtp:111 minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1\r\n/);
  assert.equal(preferStereo(stereo), stereo);
  assert.equal(preferStereo("v=0\r\n"), "v=0\r\n");
});
