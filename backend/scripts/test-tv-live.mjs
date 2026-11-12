// Live end-to-end check of Sonar TV between two accounts on a local server:
// the desktop app (development build, isolated data folder) signs in, joins a
// voice room and shares a channel from a local "provider"; Edge, signed in to the
// web app as the other account in the same room, watches it.
// Usage (from backend/): node scripts/test-tv-live.mjs [path-to-desktop-debug-exe]
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { MongoMemoryServer } from "mongodb-memory-server";
import { chromium } from "playwright";

const exe = resolve(process.argv[2] || "../desktop/target/debug/sonar-desktop.exe");
const work = mkdtempSync(join(tmpdir(), "sonar-tv-live-"));
const say = (...parts) => console.log(new Date().toISOString().slice(11, 19), ...parts);

// A 720p25 channel without B-frames (so the browser gets the original), with a moving box and a tone.
const channel = join(work, "channel.ts");
const made = spawnSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=black:s=1280x720:r=25:d=90[bg];color=white:s=40x40:d=90[b];[bg][b]overlay=x='mod(n*6\\,1200)':y=340",
  "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=90", "-c:v", "libx264", "-preset", "veryfast", "-profile:v", "high", "-pix_fmt", "yuv420p", "-g", "25", "-bf", "0", "-b:v", "3M", "-c:a", "aac", "-ac", "2", "-f", "mpegts", channel]);
if (made.status !== 0) throw new Error("ffmpeg failed");
const bytes = readFileSync(channel);
const provider = createServer((req, res) => {
  if (req.url === "/list.m3u") { res.end(`#EXTM3U\n#EXTINF:-1 group-title="Test",Test Channel HD\nhttp://127.0.0.1:${provider.address().port}/live/sonar-test-user/sonar-secret-9f3/1.ts\n`); return; }
  if (req.url.startsWith("/live/")) { res.writeHead(200, { "Content-Length": bytes.length }); res.end(bytes); return; }
  res.writeHead(404).end();
});
await new Promise((r) => provider.listen(0, "127.0.0.1", r));

const database = await MongoMemoryServer.create();
const PORT = 5333;
const origin = `http://localhost:${PORT}`;
Object.assign(process.env, { MONGODB_URI: database.getUri("sonar-tv-live"), MONGODB_DNS_SERVERS: "", NODE_ENV: "production", PORT: String(PORT), FRONTEND_URL: origin, JWT_SECRET: "tv-live-secret-tv-live-secret-tv-live-123" });
const relayed = [];
const { io } = await import("../src/lib/socket.js");
await import("../src/index.js");
await new Promise((r) => setTimeout(r, 2000));
// Record every signal the server relays, to prove the stream address never passes through it.
io.on("connection", (socket) => socket.onAny((event, payload) => relayed.push(JSON.stringify([event, payload]))));
const { default: Community } = await import("../src/models/community.model.js");
const signup = async (fullName, email, username) => {
  const response = await fetch(`${origin}/api/auth/signup`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ fullName, email, username, password: "correct-horse-battery" }) });
  if (!response.ok) throw new Error(`signup ${response.status}`);
  return (await response.json())._id;
};
const host = await signup("Tiago Host", "host@example.invalid", "tiagohost");
const guest = await signup("Ana Guest", "guest@example.invalid", "anaguest");
await Community.create({ name: "TvTest", ownerId: host, members: [host, guest], channels: [{ name: "general" }, { name: "Lounge", type: "voice" }] });
say("server ready with two accounts and a voice room");

const browser = await chromium.launch({ headless: true, executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
page.on("pageerror", (e) => say("web error:", String(e).slice(0, 160)));
await page.goto(`${origin}/login`);
await page.fill('input[autocomplete="email"]', "guest@example.invalid");
await page.fill('input[autocomplete="current-password"]', "correct-horse-battery");
await page.click("button.primary");
await page.waitForSelector(".recovery-confirm input", { timeout: 30000 });
await page.check(".recovery-confirm input");
await page.click("text=Continue to Sonar");
await page.waitForTimeout(1500);
// New accounts with no friends see the People panel first; clicking outside closes it.
if (await page.$(".modal-backdrop")) await page.click(".modal-backdrop", { position: { x: 8, y: 8 } });
await page.click('[title="Communities"]');
await page.click('[title="TvTest"]');
await page.click('[aria-label="Join voice room Lounge"]');
await page.waitForSelector('[aria-label="Connected to Lounge"]', { timeout: 20000 });
say("web viewer is in the voice room");

const data = join(work, "localappdata");
const script = ["wait:9000", "dismissrecovery:", "wait:1000", "mode:community", "wait:1500", "community:TvTest", "wait:3000", "room:Lounge", "wait:5000",
  `tvadd:http://127.0.0.1:${provider.address().port}/list.m3u`, "wait:3000", "tvplay:0,share", "wait:40000", `shot:${join(work, "desktop.png")}`, "leave:", "wait:1000"].join(";");
const desktop = spawn(exe, [], { env: { ...process.env, LOCALAPPDATA: data, SONAR_DEV_SERVER: origin, SONAR_DEV_EMAIL: "host@example.invalid", SONAR_DEV_PASSWORD: "correct-horse-battery", SONAR_DEV_SCRIPT: script, SONAR_DEV_OFFSCREEN: "1", SONAR_DEV_SILENT_AUDIO: "1" }, stdio: "ignore" });
say("desktop app started");

let result = { error: "the TV share never appeared" };
try {
  await page.waitForSelector("figcaption >> text=/is sharing TV/", { timeout: 70000 });
  say("web viewer sees:", await page.textContent(".screen-tile figcaption span"));
  await page.waitForFunction(() => document.querySelector(".screen-tile video")?.videoWidth > 0, null, { timeout: 20000 });
  const first = await page.evaluate(() => document.querySelector(".screen-tile video").getVideoPlaybackQuality().totalVideoFrames);
  await page.waitForTimeout(8000);
  result = await page.evaluate((first) => {
    const video = document.querySelector(".screen-tile video");
    const quality = video.getVideoPlaybackQuality();
    return { label: document.querySelector(".screen-tile figcaption span").textContent, size: `${video.videoWidth}x${video.videoHeight}`, framesIn8s: quality.totalVideoFrames - first, dropped: quality.droppedVideoFrames, theater: Boolean(document.querySelector(".screen-stage.theater")), chatHidden: !document.querySelector(".community-content")?.offsetParent };
  }, first);
  await page.screenshot({ path: join(work, "web.png") });
} catch (error) { result.detail = String(error).slice(0, 200); }
const leaked = relayed.filter((line) => line.includes("sonar-secret-9f3") || line.includes("sonar-test-user") || line.includes("/live/"));
result.signalsRelayed = relayed.length;
result.addressSeenByServer = leaked.length > 0;
console.log(JSON.stringify(result, null, 1));
console.log("screenshots:", work);
desktop.kill();
await browser.close();
provider.close();
process.exit(result.error || result.addressSeenByServer ? 1 : 0);
