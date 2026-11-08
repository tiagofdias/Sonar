// Checks TURN allocation only; it does not capture audio or establish a call.
export function testVoiceRelay(iceServers, { timeoutMs = 10000, Peer = RTCPeerConnection } = {}) {
  let pc, timer, finish;
  const promise = new Promise((resolve) => {
    let done = false;
    finish = (result) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      if (pc) {
        pc.onicecandidate = null;
        pc.onicegatheringstatechange = null;
        pc.close();
      }
      resolve(result);
    };
    try {
      pc = new Peer({ iceServers, iceTransportPolicy: "relay" });
      pc.onicecandidate = ({ candidate }) => {
        if (candidate?.type === "relay" || /(?:^|\s)typ relay(?:\s|$)/.test(candidate?.candidate || "")) finish("available");
        else if (!candidate) finish("unavailable");
      };
      pc.onicegatheringstatechange = () => {
        if (pc.iceGatheringState === "complete") finish("unavailable");
      };
      timer = setTimeout(() => finish("unavailable"), timeoutMs);
      pc.createDataChannel("sonar-connectivity-test");
      pc.createOffer().then(offer => pc.setLocalDescription(offer)).catch(() => finish("unavailable"));
    } catch {
      finish("unavailable");
    }
  });
  return { promise, cancel: () => finish("cancelled") };
}
