// Live end-to-end check of Listen together: the desktop app (development build) plays the
// sound of one app on this PC for a voice room; Edge, signed in to the web app as another
// member, hears it straight away (no join step), in stereo, and sees what is playing.
// The "music app" is ffplay with a very quiet tone in the left channel only, in a window
// named like a song; renaming the window stands in for the next song.
// Usage (from backend/): node scripts/test-music-live.mjs [path-to-desktop-debug-exe]
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { MongoMemoryServer } from "mongodb-memory-server";
import { chromium } from "playwright";

const exe = resolve(process.argv[2] || "../desktop/target/debug/sonar-desktop.exe");
const work = mkdtempSync(join(tmpdir(), "sonar-music-live-"));
const say = (...parts) => console.log(new Date().toISOString().slice(11, 19), ...parts);

// Left: a 440 Hz tone. Right: silence. Volume 3 of 100, so it is barely audible here.
const app = spawn("ffplay", ["-hide_banner", "-loglevel", "error", "-autoexit", "-volume", "3", "-window_title", "Test Artist - First Song", "-f", "lavfi",
  "color=c=0x203040:s=320x180:d=150[out0];sine=frequency=440:sample_rate=48000:duration=150,pan=stereo|c0=c0|c1=0*c0[out1]"], { stdio: "ignore" });
await new Promise((r) => setTimeout(r, 2000));

const database = await MongoMemoryServer.create();
const PORT = 5338;
const origin = `http://localhost:${PORT}`;
Object.assign(process.env, { MONGODB_URI: database.getUri("sonar-music-live"), MONGODB_DNS_SERVERS: "", NODE_ENV: "production", PORT: String(PORT), FRONTEND_URL: origin, JWT_SECRET: "music-live-secret-music-live-secret-1234" });
const relayed = [];
const { io } = await import("../src/lib/socket.js");
await import("../src/index.js");
await new Promise((r) => setTimeout(r, 2000));
io.on("connection", (socket) => socket.onAny((event, payload) => relayed.push(JSON.stringify([event, payload]))));
const { default: Community } = await import("../src/models/community.model.js");
const signup = async (fullName, email, username) => {
  const response = await fetch(`${origin}/api/auth/signup`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ fullName, email, username, password: "correct-horse-battery" }) });
  if (!response.ok) throw new Error(`signup ${response.status}`);
  return (await response.json())._id;
};
const host = await signup("Tiago Host", "host@example.invalid", "tiagohost");
const guest = await signup("Ana Guest", "guest@example.invalid", "anaguest");
await Community.create({ name: "MusicTest", ownerId: host, members: [host, guest], channels: [{ name: "general" }, { name: "Lounge", type: "voice" }] });
say("server ready");

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
if (await page.$(".modal-backdrop")) await page.click(".modal-backdrop", { position: { x: 8, y: 8 } });
await page.click('[title="Communities"]');
await page.click('[title="MusicTest"]');
await page.click('[aria-label="Join voice room Lounge"]');
await page.waitForSelector('[aria-label^="Connected"]', { timeout: 20000 });
say("web listener is in the voice room");

const script = ["wait:9000", "dismissrecovery:", "wait:1000", "mode:community", "wait:1500", "community:MusicTest", "wait:3000", "room:Lounge", "wait:5000",
  "music", "wait:2500", "musicapp:First Song", "wait:20000", `shot:${join(work, "desktop.png")}`, "wait:20000", "leave:", "wait:10000"].join(";");
const desktop = spawn(exe, [], { env: { ...process.env, LOCALAPPDATA: join(work, "data"), SONAR_DEV_SERVER: origin, SONAR_DEV_EMAIL: "host@example.invalid", SONAR_DEV_PASSWORD: "correct-horse-battery", SONAR_DEV_SCRIPT: script, SONAR_DEV_OFFSCREEN: "1", SONAR_DEV_SILENT_AUDIO: "1" }, stdio: "ignore" });
say("desktop app started");

let result = { error: "the music never arrived" };
try {
  await page.waitForSelector(".music-bar", { timeout: 70000 });
  result = { joinButtonShown: Boolean(await page.$("button.apps-join")), title: await page.textContent(".music-bar strong") };
  // Measure each channel of what the listener hears.
  await page.waitForFunction(() => document.querySelector(".music-bar audio")?.srcObject?.getAudioTracks().length > 0, null, { timeout: 20000 });
  await page.evaluate(() => {
    const stream = document.querySelector(".music-bar audio").srcObject;
    const context = new AudioContext();
    const split = context.createChannelSplitter(2);
    context.createMediaStreamSource(stream).connect(split);
    window.levels = [0, 0];
    [0, 1].forEach((channel) => {
      const analyser = context.createAnalyser();
      split.connect(analyser, channel);
      const data = new Float32Array(2048);
      setInterval(() => { analyser.getFloatTimeDomainData(data); let sum = 0; for (const v of data) sum += v * v; window.levels[channel] = Math.max(window.levels[channel], Math.sqrt(sum / data.length)); }, 50);
    });
  });
  await page.waitForTimeout(4000);
  const [left, right] = await page.evaluate(() => window.levels);
  result.left = Number(left.toFixed(5)); result.right = Number(right.toFixed(5));
  result.stereo = left > 0.0005 && right < left / 10;
  await page.screenshot({ path: join(work, "web.png") });
  // The next song: the music app's window title changes.
  execFileSync("powershell", ["-NoProfile", "-Command", `Add-Type 'using System; using System.Runtime.InteropServices; public static class W { [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr h, string t); }'; [W]::SetWindowText((Get-Process -Id ${app.pid}).MainWindowHandle, 'Test Artist - Second Song') | Out-Null`]);
  await page.waitForSelector(".music-bar strong >> text=/Second Song/", { timeout: 15000 });
  result.titleFollows = true;
  // The host leaves: the music stops for everyone.
  await page.waitForSelector(".music-bar", { state: "detached", timeout: 60000 });
  result.endsWhenHostLeaves = true;
} catch (error) { result.detail = String(error).slice(0, 200); }
result.signalsRelayed = relayed.length;
console.log(JSON.stringify(result, null, 1));
console.log("screenshots:", work);
desktop.kill(); app.kill();
await browser.close();
process.exit(result.error || result.detail || result.joinButtonShown || !result.stereo || !result.titleFollows || !result.endsWhenHostLeaves ? 1 : 0);
