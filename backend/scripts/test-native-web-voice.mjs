// Local synthetic audio interoperability; no microphone or database is used.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { createInterface } from "node:readline";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const executable = process.env.SONAR_VOICE_PROBE || resolve(root, "desktop/target/debug/examples/voice_browser_probe.exe");
const server = createServer((_req, res) => { res.writeHead(200, { "content-type": "text/html" }); res.end("<!doctype html><title>Sonar voice test</title>"); });
let browser, child;
try {
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  browser = await chromium.launch({ headless: true, args: ["--autoplay-policy=no-user-gesture-required", "--disable-features=WebRtcHideLocalIpsWithMdns"], ...(process.env.SONAR_BROWSER_EXECUTABLE ? { executablePath: process.env.SONAR_BROWSER_EXECUTABLE } : {}) });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  child = spawn(executable, [], { stdio: ["pipe", "pipe", "pipe"] });
  let stderr = ""; child.stderr.on("data", chunk => { stderr += chunk; });
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const next = async () => {
    let timer;
    try {
      const result = await Promise.race([lines.next(), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Native voice probe timed out")), 30000); })]);
      if (result.done) throw new Error(`Native voice probe stopped: ${stderr}`);
      return JSON.parse(result.value);
    } finally { clearTimeout(timer); }
  };
  const offer = await next();
  assert.equal(offer.type, "offer");
  const answer = await page.evaluate(async offer => {
    const pc = new RTCPeerConnection({ iceServers: [] });
    const candidates = [];
    pc.onicecandidate = event => { if (event.candidate) candidates.push(event.candidate.toJSON()); };
    const context = new AudioContext({ sampleRate: 48000 });
    const oscillator = context.createOscillator(); oscillator.frequency.value = 660;
    const gain = context.createGain(); gain.gain.value = 0.2;
    const stream = context.createMediaStreamDestination();
    oscillator.connect(gain).connect(stream); oscillator.start(); await context.resume();
    pc.ontrack = event => {
      const remote = event.streams[0] || new MediaStream([event.track]);
      const playback = new Audio(); playback.autoplay = true; playback.muted = true; playback.srcObject = remote; playback.play(); window.__sonarPlayback = playback;
      const source = context.createMediaStreamSource(remote);
      const analyser = context.createAnalyser(); analyser.fftSize = 1024;
      const sink = context.createGain(); sink.gain.value = 0.001;
      source.connect(analyser).connect(sink).connect(context.destination);
      window.__sonarPeak = 0;
      window.__sonarMeter = setInterval(() => {
        const samples = new Float32Array(analyser.fftSize); analyser.getFloatTimeDomainData(samples);
        window.__sonarPeak = Math.max(window.__sonarPeak, ...samples.map(Math.abs));
      }, 20);
    };
    pc.addTrack(stream.stream.getAudioTracks()[0], stream.stream);
    await pc.setRemoteDescription({ type: "offer", sdp: offer.sdp });
    await pc.setLocalDescription(await pc.createAnswer());
    if (pc.iceGatheringState !== "complete") await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Browser ICE timeout")), 10000);
      pc.addEventListener("icegatheringstatechange", () => { if (pc.iceGatheringState === "complete") { clearTimeout(timer); resolve(); } });
    });
    const fingerprint = pc.localDescription.sdp.match(/^a=fingerprint:sha-256 ([0-9A-F:]+)\r?$/im)?.[1];
    if (!fingerprint) throw new Error("Browser fingerprint missing");
    window.__sonarPeer = pc; window.__sonarAudio = { context, oscillator, stream };
    if (!candidates.length) throw new Error("Browser did not gather a trickle candidate");
    const sdp = pc.localDescription.sdp.split(/\r?\n/).filter(line => !line.startsWith("a=candidate:") && line !== "a=end-of-candidates").join("\r\n");
    return { sdp, fingerprint, candidates };
  }, offer);
  child.stdin.end(JSON.stringify(answer) + "\n");
  const result = await next();
  assert.equal(result.type, "result");
  assert.ok(result.receivedPackets > 5, `Native peer received ${result.receivedPackets} packets`);
  assert.ok(result.peak > 0.01, `Native decoded peak was ${result.peak}`);
  const stats = await page.evaluate(async () => {
    const reports = await window.__sonarPeer.getStats();
    return [...reports.values()].filter(item => item.type === "inbound-rtp" && item.kind === "audio").map(item => ({ packets: item.packetsReceived || 0, bytes: item.bytesReceived || 0, samples: item.totalSamplesReceived || 0, energy: item.totalAudioEnergy || 0, codec: reports.get(item.codecId)?.mimeType, jitter: item.jitterBufferEmittedCount, discarded: item.packetsDiscarded, conceal: item.concealedSamples, track: window.__sonarPeer.getReceivers().map(r => ({ muted: r.track.muted, state: r.track.readyState })), audioState: window.__sonarAudio.context.state }));
  });
  assert.ok(stats.some(item => item.packets > 5 && item.bytes > 0), "Browser did not receive native audio packets");
  const browserPeak = await page.evaluate(() => window.__sonarPeak || 0);
  assert.ok(browserPeak > 0.01, `Browser did not decode audible native audio (peak ${browserPeak}): ${JSON.stringify(stats)}`);
  await page.evaluate(async () => { clearInterval(window.__sonarMeter); window.__sonarPeer.close(); window.__sonarAudio.oscillator.stop(); window.__sonarAudio.stream.stream.getTracks().forEach(track => track.stop()); await window.__sonarAudio.context.close(); });
  console.log("PASS: native Rust and browser exchanged synthetic Opus over local WebRTC in both directions.");
} finally {
  child?.kill();
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
