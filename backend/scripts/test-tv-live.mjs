// Live end-to-end check of Sonar TV between two accounts on a local server:
// the desktop app (development build, isolated data folder) signs in, joins a
// voice room and shares a channel from a local "provider"; Edge, signed in to the
// web app as the other account in the same room, watches it.
// Usage (from backend/): node scripts/test-tv-live.mjs [path-to-desktop-debug-exe]
// SONAR_TV_CHANNEL=<http(s) stream URL> shares that real stream instead of the generated one.
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
const real = process.env.SONAR_TV_CHANNEL;
const provider = createServer((req, res) => {
  const local = (n) => `http://127.0.0.1:${provider.address().port}/live/sonar-test-user/sonar-secret-9f3/${n}.ts`;
  if (req.url === "/list.m3u") { res.end(`#EXTM3U\n#EXTINF:-1 group-title="Test",Test Channel HD\n${real || local(1)}\n#EXTINF:-1 group-title="Test",Second Channel\n${local(2)}\n`); return; }
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
const third = await signup("Bea Late", "late@example.invalid", "bealate");
await Community.create({ name: "TvTest", ownerId: host, members: [host, guest, third], channels: [{ name: "general" }, { name: "Lounge", type: "voice" }] });
say("server ready with two accounts and a voice room");

const browser = await chromium.launch({ headless: true, executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"] });
// Signs a web member in and puts them in the voice room.
const enterRoom = async (email) => {
  const tab = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
  tab.on("pageerror", (e) => say("web error:", String(e).slice(0, 160)));
  await tab.goto(`${origin}/login`);
  await tab.fill('input[autocomplete="email"]', email);
  await tab.fill('input[autocomplete="current-password"]', "correct-horse-battery");
  await tab.click("button.primary");
  await tab.waitForSelector(".recovery-confirm input", { timeout: 30000 });
  await tab.check(".recovery-confirm input");
  await tab.click("text=Continue to Sonar");
  await tab.waitForTimeout(1500);
  // New accounts with no friends see the People panel first; clicking outside closes it.
  if (await tab.$(".modal-backdrop")) await tab.click(".modal-backdrop", { position: { x: 8, y: 8 } });
  await tab.click('[title="Communities"]');
  await tab.click('[title="TvTest"]');
  await tab.click('[aria-label="Join voice room Lounge"]');
  await tab.waitForSelector('[aria-label^="Connected"]', { timeout: 20000 });
  return tab;
};
const page = await enterRoom("guest@example.invalid");
say("web viewer is in the voice room");

const data = join(work, "localappdata");
const script = ["wait:9000", "dismissrecovery:", "wait:1000", "mode:community", "wait:1500", "community:TvTest", "wait:3000", "room:Lounge", "wait:5000",
  `tvadd:http://127.0.0.1:${provider.address().port}/list.m3u`, "wait:3000", "tvplay:0,share", "wait:15000", `shot:${join(work, "desktop.png")}`, "wait:50000", "tvnext:1", "wait:30000", "leave:", "wait:15000"].join(";");
const desktop = spawn(exe, [], { env: { ...process.env, LOCALAPPDATA: data, SONAR_DEV_SERVER: origin, SONAR_DEV_EMAIL: "host@example.invalid", SONAR_DEV_PASSWORD: "correct-horse-battery", SONAR_DEV_SCRIPT: script, SONAR_DEV_OFFSCREEN: "1", SONAR_DEV_SILENT_AUDIO: "1" }, stdio: "ignore" });
say("desktop app started");

let result = { error: "the TV share never appeared" };
try {
  // TV waits for the viewer to join: a Join button, and no video until it is pressed.
  await page.waitForSelector("button.apps-join >> text=/Join TV/", { timeout: 70000 });
  const autoOpened = Boolean(await page.$(".screen-tile video"));
  await page.click('[aria-label="Apps"]');
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(work, "web-apps-menu.png") });
  await page.click('[aria-label="Apps"]');
  await page.click("button.apps-join >> text=/Join TV/");
  await page.waitForSelector("figcaption >> text=/is sharing TV/", { timeout: 20000 });
  say("web viewer sees:", await page.textContent(".screen-tile figcaption span"));
  await page.waitForFunction(() => document.querySelector(".screen-tile video")?.videoWidth > 0, null, { timeout: 20000 });
  const first = await page.evaluate(() => document.querySelector(".screen-tile video").getVideoPlaybackQuality().totalVideoFrames);
  await page.waitForTimeout(8000);
  result = await page.evaluate((first) => {
    const video = document.querySelector(".screen-tile video");
    const quality = video.getVideoPlaybackQuality();
    return { label: document.querySelector(".screen-tile figcaption span").textContent, size: `${video.videoWidth}x${video.videoHeight}`, framesIn8s: quality.totalVideoFrames - first, hasSound: video.srcObject?.getAudioTracks().length > 0, dropped: quality.droppedVideoFrames, theater: Boolean(document.querySelector(".screen-stage.theater")), chatHidden: !document.querySelector(".community-content")?.offsetParent };
  }, first);
  result.autoOpened = autoOpened;
  await page.screenshot({ path: join(work, "web.png") });
  // Someone else joins the room, then starts watching: the first viewer's picture must not stall.
  await page.evaluate(() => {
    const video = document.querySelector(".screen-tile video");
    window.frameGap = { last: performance.now(), longest: 0 };
    const each = () => { const now = performance.now(); window.frameGap.longest = Math.max(window.frameGap.longest, now - window.frameGap.last); window.frameGap.last = now; video.requestVideoFrameCallback(each); };
    video.requestVideoFrameCallback(each);
  });
  const late = await enterRoom("late@example.invalid");
  say("a third member joined the room");
  await late.waitForSelector("button.apps-join >> text=/Join TV/", { timeout: 30000 });
  await late.click("button.apps-join >> text=/Join TV/");
  await late.waitForFunction(() => document.querySelector(".screen-tile video")?.videoWidth > 0, null, { timeout: 20000 });
  say("the third member is watching too");
  await page.waitForTimeout(4000);
  result.longestStallWhileOthersJoinedMs = Math.round(await page.evaluate(() => window.frameGap.longest));
  result.lateViewerSees = await late.evaluate(() => document.querySelector(".screen-tile video").videoWidth > 0);
  // The sharer switches channel: same connection, new name, the picture keeps going.
  await page.waitForSelector("figcaption >> text=/Second Channel/", { timeout: 90000 });
  const before = await page.evaluate(() => document.querySelector(".screen-tile video").getVideoPlaybackQuality().totalVideoFrames);
  await page.waitForTimeout(5000);
  result.afterSwitch = await page.evaluate((before) => ({ label: document.querySelector(".screen-tile figcaption span").textContent, framesIn5s: document.querySelector(".screen-tile video").getVideoPlaybackQuality().totalVideoFrames - before }), before);
  await page.screenshot({ path: join(work, "web-switched.png") });
  // Leaving closes it, and it can be joined again.
  await page.click('[aria-label="Stop watching"]');
  await page.waitForSelector("button.apps-join >> text=/Join TV · Second Channel/", { timeout: 20000 });
  result.canRejoin = !(await page.$(".screen-tile video"));
  // The sharer leaves the room: TV ends for everyone, and the waiting Join button goes too.
  await late.waitForSelector(".screen-tile video", { state: "detached", timeout: 60000 });
  await page.waitForSelector("button.apps-join >> text=/Join TV/", { state: "detached", timeout: 10000 });
  result.endsWhenSharerLeaves = true;
} catch (error) { result.detail = String(error).slice(0, 200); }
const leaked = relayed.filter((line) => line.includes("sonar-secret-9f3") || line.includes("sonar-test-user") || line.includes("/live/") || (real && line.includes(new URL(real).host)));
result.signalsRelayed = relayed.length;
result.addressSeenByServer = leaked.length > 0;
console.log(JSON.stringify(result, null, 1));
console.log("screenshots:", work);
desktop.kill();
await browser.close();
provider.close();
process.exit(result.error || result.detail || result.addressSeenByServer || result.autoOpened || !result.canRejoin || !(result.afterSwitch?.framesIn5s > 0) || !(result.longestStallWhileOthersJoinedMs < 1000) || !result.endsWhenSharerLeaves ? 1 : 0);
