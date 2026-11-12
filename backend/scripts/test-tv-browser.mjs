// Browser side of the Sonar TV end-to-end check (desktop/src/screen.rs, test
// `tv_share_reaches_a_browser`). Reads the desktop's offer on stdin, answers,
// plays the share like the web app does, and on "measure" reports what Edge saw:
// frames decoded, picture size, whether a moving box ever went backwards, sound.
import { createInterface } from "node:readline";
import { chromium } from "playwright";

const browser = await chromium.launch({ headless: true, executablePath: process.env.SONAR_BROWSER_EXECUTABLE || "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage();
await page.goto("about:blank");
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const offer = JSON.parse((await lines.next()).value);
const answer = await page.evaluate(async (offer) => {
  const pc = new RTCPeerConnection();
  window.pc = pc;
  window.shown = [];
  window.peak = 0;
  pc.ontrack = ({ track, streams }) => {
    const stream = streams[0] || new MediaStream([track]);
    if (track.kind === "audio") {
      const context = new AudioContext();
      const analyser = context.createAnalyser();
      context.createMediaStreamSource(new MediaStream([track])).connect(analyser);
      const data = new Float32Array(2048);
      setInterval(() => { analyser.getFloatTimeDomainData(data); for (const v of data) window.peak = Math.max(window.peak, Math.abs(v)); }, 50);
      const sink = new Audio(); sink.muted = true; sink.srcObject = new MediaStream([track]); sink.play().catch(() => {});
      return;
    }
    const video = document.createElement("video");
    video.muted = true; video.autoplay = true; video.playsInline = true; video.srcObject = new MediaStream([track]);
    document.body.appendChild(video);
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    const each = (_, meta) => {
      canvas.width = meta.width; canvas.height = meta.height;
      ctx.drawImage(video, 0, 0);
      const row = ctx.getImageData(0, Math.round(meta.height * 0.5), meta.width, 1).data;
      let x = -1; for (let i = 0; i < meta.width; i++) if (row[i * 4] > 128) { x = i; break; }
      window.shown.push({ x, w: meta.width, h: meta.height });
      video.requestVideoFrameCallback(each);
    };
    video.requestVideoFrameCallback(each);
    video.play().catch(() => {});
  };
  await pc.setRemoteDescription(offer);
  await pc.setLocalDescription(await pc.createAnswer());
  await new Promise((resolve) => { if (pc.iceGatheringState === "complete") resolve(); pc.onicegatheringstatechange = () => pc.iceGatheringState === "complete" && resolve(); });
  return pc.localDescription.toJSON();
}, offer);
process.stdout.write(JSON.stringify(answer) + "\n");
await lines.next(); // "measure"
await page.waitForTimeout(800);
const result = await page.evaluate(async () => {
  const stats = [...(await window.pc.getStats()).values()];
  const video = stats.find((s) => s.type === "inbound-rtp" && s.kind === "video") || {};
  const audio = stats.find((s) => s.type === "inbound-rtp" && s.kind === "audio") || {};
  const xs = window.shown.map((s) => s.x).filter((x) => x >= 0);
  const steps = xs.slice(1).map((x, i) => x - xs[i]);
  return {
    framesDecoded: video.framesDecoded || 0, framesDropped: video.framesDropped || 0, freezeCount: video.freezeCount || 0,
    size: `${video.frameWidth}x${video.frameHeight}`, framesShown: window.shown.length,
    boxBackwards: steps.filter((d) => d < 0 && d > -1000).length, boxSteps: steps.length,
    audioPackets: audio.packetsReceived || 0, audioChannels: audio.channels, soundPeak: window.peak,
  };
});
process.stdout.write(JSON.stringify(result) + "\n");
await browser.close();
process.exit(0);
