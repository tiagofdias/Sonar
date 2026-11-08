// Isolated browser smoke test. Never connects to the developer's configured database.
import assert from "node:assert/strict";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, resolve } from "node:path";
import { mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import bcrypt from "bcryptjs";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { chromium } from "playwright";
import { app, server, io, allowedOrigins } from "../src/lib/socket.js";
import authRoutes from "../src/routes/auth.route.js";
import messageRoutes from "../src/routes/message.route.js";
import communityRoutes from "../src/routes/community.route.js";
import voiceRoutes from "../src/routes/voice.route.js";
import User from "../src/models/user.model.js";
import Message from "../src/models/message.model.js";
import Community from "../src/models/community.model.js";
import ChannelMessage from "../src/models/channelMessage.model.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const frontendRequire = createRequire(resolve(root, "frontend/package.json"));
const { createServer } = await import(
  pathToFileURL(frontendRequire.resolve("vite")).href
);
const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
let vite, browser;
try {
  await mongoose.connect(database.getUri("sonar-browser-tests"));
  await User.create({
    fullName: "Browser tester",
    email: "browser@example.invalid",
    password: await bcrypt.hash("TenChars!!", 4),
  });
  app.use(express.json({ limit: "800kb" }));
  app.use(cookieParser());
  app.use("/api/auth", authRoutes);
  app.use("/api/messages", messageRoutes);
  app.use("/api/communities", communityRoutes);
  app.use("/api/voice", voiceRoutes);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const backend = `http://127.0.0.1:${server.address().port}`;
  const frontend = "http://127.0.0.1:5174";
  process.chdir(resolve(root, "frontend"));
  process.env.WEBAUTHN_ORIGIN = "http://localhost:5174";
  allowedOrigins.push(frontend, process.env.WEBAUTHN_ORIGIN);
  vite = await createServer({
    root: resolve(root, "frontend"),
    configFile: resolve(root, "frontend/vite.config.js"),
    server: {
      host: "127.0.0.1",
      port: 5174,
      strictPort: true,
      proxy: { "/api": backend, "/socket.io": { target: backend, ws: true } },
    },
  });
  await vite.listen();
  const executablePath = process.env.SONAR_BROWSER_EXECUTABLE;
  browser = await chromium.launch({
    headless: true,
    args: [
      "--use-fake-device-for-media-stream",
      "--use-fake-ui-for-media-stream",
      "--autoplay-policy=no-user-gesture-required",
    ],
    ...(executablePath ? { executablePath } : {}),
  });
  const saveRecoveryKey = async (target) => {
    await target.getByRole("heading", { name: "Save your recovery key." }).waitFor();
    const key = await target.locator(".recovery-key").innerText();
    assert.match(key, /^([0-9A-Z]{4}-){5}[0-9A-Z]{4}$/);
    assert.equal(await target.getByRole("button", { name: /Continue to Sonar/ }).isDisabled(), true);
    await target.getByLabel("I saved my recovery key somewhere safe").check();
    await target.getByRole("button", { name: /Continue to Sonar/ }).click();
    return key;
  };
  const context = await browser.newContext({
    permissions: ["microphone"],
    viewport: { width: 1280, height: 1050 },
  });
  const page = await context.newPage();
  const instrumentation = () => {
    if (!navigator.mediaDevices?.getUserMedia) return;
    window.__sonarTestConnections = [];
    window.__sonarTestTracks = [];
    window.__sonarTestAudioElements = [];
    const OriginalAudio = window.Audio;
    window.Audio = function (...args) {
      const element = new OriginalAudio(...args);
      window.__sonarTestAudioElements.push(element);
      return element;
    };
    window.Audio.prototype = OriginalAudio.prototype;
    const Original = window.RTCPeerConnection;
    window.RTCPeerConnection = class extends Original {
      constructor(...args) {
        super(...args);
        window.__sonarTestConnections.push(this);
      }
    };
    const capture = navigator.mediaDevices.getUserMedia.bind(
      navigator.mediaDevices,
    );
    navigator.mediaDevices.getUserMedia = async (constraints) => {
      const stream = await capture(constraints);
      window.__sonarTestTracks.push(...stream.getTracks());
      return stream;
    };
  };
  await page.addInitScript(instrumentation);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(frontend);
  console.log("Browser: opening login");
  await page
    .getByLabel("Email address", { exact: true })
    .fill("browser@example.invalid");
  await page.getByLabel("Password", { exact: true }).fill("TenChars!!");
  await page.getByRole("button", { name: "Enter your space" }).click();
  await saveRecoveryKey(page);
  const tester = await User.findOne({ email: "browser@example.invalid" });
  await page.getByTitle("Your account", { exact: true }).waitFor();
  const friendKeys = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveKey"],
  );
  const friend = await User.create({
    fullName: "History friend",
    email: "history-friend@example.invalid",
    password: await bcrypt.hash("TenChars!!", 4),
    encryptionKey: Buffer.from(
      await crypto.subtle.exportKey("raw", friendKeys.publicKey),
    ).toString("base64"),
  });
  const envelopes = await page.evaluate(
    async ({ me, peer }) => {
      const { encrypt } = await import("/src/lib/crypto.js");
      return Promise.all(
        Array.from({ length: 60 }, (_, index) =>
          encrypt(
            me,
            peer,
            `History message ${String(index).padStart(3, "0")}`,
          ),
        ),
      );
    },
    {
      me: String(tester._id),
      peer: { _id: String(friend._id), encryptionKey: friend.encryptionKey },
    },
  );
  await Message.insertMany(
    envelopes.map((envelope, index) => ({
      ...envelope,
      senderId: tester._id,
      receiverId: friend._id,
      pinned: index === 0,
      createdAt: new Date(Date.now() - 60000),
      expiresAt: new Date(Date.now() + 86400000),
    })),
  );
  await page.reload();
  const friendButton = () =>
    page.locator(".people-list .person").filter({ hasText: "History friend" });
  await friendButton().click();
  await page.locator("article.message").nth(49).waitFor();
  assert.equal(await page.locator("article.message").count(), 50);
  await page.locator(".message-scroll").evaluate((node) => {
    node.scrollTop = 0;
  });
  const anchor = page.getByText("History message 010", { exact: true });
  const before = (await anchor.boundingBox()).y;
  await page
    .getByRole("button", { name: "Load older messages", exact: true })
    .click();
  await page.waitForFunction(
    () => document.querySelectorAll("article.message").length === 60,
  );
  await page.evaluate(
    () =>
      new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      ),
  );
  assert.ok(
    Math.abs((await anchor.boundingBox()).y - before) < 3,
    "older pages preserve the visible message position",
  );
  await page.getByTitle("Pinned messages", { exact: true }).click();
  await page.getByText("History message 000", { exact: true }).waitFor();
  assert.equal(await page.locator("article.message").count(), 1);
  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Delete message" }).click();
  await page.getByText("No pinned messages yet.").waitFor();
  assert.equal(await Message.countDocuments({ senderId: tester._id, receiverId: friend._id, ciphertext: envelopes[0].ciphertext }), 0);
  await page.getByTitle("Pinned messages", { exact: true }).click();
  await page.locator("article.message").nth(49).waitFor();
  await page
    .getByLabel("Message", { exact: true })
    .fill("Private unsent draft");
  await page.getByTitle("Sonar home", { exact: true }).click();
  await friendButton().click();
  assert.equal(
    await page.getByLabel("Message", { exact: true }).inputValue(),
    "Private unsent draft",
  );
  await page.getByLabel("Message", { exact: true }).fill("");
  console.log(
    "PASS: paginated encrypted history, old pins, scroll anchoring, and conversation drafts.",
  );
  const newKeys = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveKey"]);
  const replacement = Buffer.from(await crypto.subtle.exportKey("raw", newKeys.publicKey)).toString("base64");
  await User.updateOne({ _id: friend._id }, { encryptionKey: replacement });
  await page.evaluate(async () => { const { useChatStore } = await import("/src/store/useChatStore.js"); await useChatStore.getState().getUsers(); });
  await page.getByTitle("Conversation details", { exact: true }).click();
  await page.getByText("safety code", { exact: false }).first().waitFor();
  await page.getByRole("button", { name: "The codes match, trust the new key" }).click();
  await page.getByText("New key trusted on this device. Reload the conversation to refresh its history.").waitFor();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  console.log("PASS: changed contact key requires explicit safety-code confirmation.");
  const crew = await Community.create({
    name: "History crew",
    ownerId: tester._id,
    members: [tester._id],
    channels: [{ name: "general" }, { name: "sandbox" }],
  });
  const currentTester = await User.findById(tester._id);
  const channelEnvelopes = await page.evaluate(
    async ({ me, peer, channelId }) => {
      const { encrypt } = await import("/src/lib/crypto.js");
      return Promise.all(
        Array.from({ length: 60 }, (_, index) =>
          encrypt(
            me,
            peer,
            `Channel message ${String(index).padStart(3, "0")}`,
            channelId,
          ),
        ),
      );
    },
    {
      me: String(tester._id),
      peer: {
        _id: String(tester._id),
        encryptionKey: currentTester.encryptionKey,
      },
      channelId: String(crew.channels[0]._id),
    },
  );
  await ChannelMessage.insertMany(
    channelEnvelopes.map((envelope, index) => ({
      senderId: tester._id,
      communityId: crew._id,
      channelId: crew.channels[0]._id,
      envelopes: [{ recipientId: tester._id, ...envelope }],
      pinned: index === 0,
      createdAt: new Date(Date.now() - 60000),
      expiresAt: new Date(Date.now() + 86400000),
    })),
  );
  await page.getByTitle("Communities", { exact: true }).click();
  await page.getByTitle("History crew", { exact: true }).click();
  await page.locator("article.message").nth(49).waitFor();
  await page
    .getByRole("button", { name: "Load older messages", exact: true })
    .click();
  await page.waitForFunction(
    () => document.querySelectorAll("article.message").length === 60,
  );
  await page.getByTitle("Pinned messages", { exact: true }).click();
  await page.getByText("Channel message 000", { exact: true }).waitFor();
  assert.equal(await page.locator("article.message").count(), 1);
  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Delete message" }).click();
  await page.getByText("No pinned messages yet.").waitFor();
  assert.equal(await ChannelMessage.countDocuments({ communityId: crew._id, pinned: true }), 0);
  await page.getByTitle("Pinned messages", { exact: true }).click();
  await page
    .getByLabel("Message general", { exact: true })
    .fill("General channel draft");
  await page.getByRole("button", { name: "sandbox", exact: true }).click();
  await page
    .getByLabel("Message sandbox", { exact: true })
    .fill("Sandbox channel draft");
  await page.getByRole("button", { name: "general", exact: true }).click();
  assert.equal(
    await page.getByLabel("Message general", { exact: true }).inputValue(),
    "General channel draft",
  );
  await page.getByRole("button", { name: "Delete a channel or room" }).click();
  await page.getByLabel("Channel or room").selectOption({ label: "sandbox (text)" });
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await page.getByRole("button", { name: "Delete channel", exact: true }).click();
  await page.getByText("Channel deleted", { exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "sandbox", exact: true }).count(), 0);
  await page
    .getByRole("button", { name: "Back to messages", exact: true })
    .click();
  console.log(
    "PASS: channel pagination, old pins, and isolated channel drafts.",
  );
  await User.create({
    fullName: "Voice friend",
    email: "voice-peer@example.invalid",
    password: await bcrypt.hash("TenChars!!", 4),
  });
  const otherContext = await browser.newContext({
    permissions: ["microphone"],
    viewport: { width: 1280, height: 1050 },
  });
  const otherPage = await otherContext.newPage();
  await otherPage.addInitScript(instrumentation);
  await otherPage.goto(frontend);
  await otherPage
    .getByLabel("Email address", { exact: true })
    .fill("voice-peer@example.invalid");
  await otherPage.getByLabel("Password", { exact: true }).fill("TenChars!!");
  await otherPage
    .getByRole("button", { name: "Enter your space", exact: true })
    .click();
  await saveRecoveryKey(otherPage);
  await otherPage.getByTitle("Your account", { exact: true }).waitFor();
  await page.evaluate(async () => {
    const { useChatStore } = await import("/src/store/useChatStore.js");
    await useChatStore.getState().getUsers();
  });
  await otherPage.locator(".people-list .person").filter({ hasText: "Browser tester" }).click();
  await otherPage.getByLabel("Message", { exact: true }).fill("A new private hello");
  await otherPage.getByLabel("Message", { exact: true }).press("Enter");
  const friendEntry = page.locator(".people-list .person").filter({ hasText: "Voice friend" });
  await friendEntry.getByLabel("New messages").waitFor();
  await page
    .locator(".people-list .person")
    .filter({ hasText: "Voice friend" })
    .click();
  assert.equal(await friendEntry.getByLabel("New messages").count(), 0);
  await page.locator("article.message").filter({ hasText: "A new private hello" }).waitFor();
  await page.waitForFunction(async () => {
    const { useAuthStore } = await import("/src/store/useAuthStore.js");
    const { useChatStore } = await import("/src/store/useChatStore.js");
    const { readPosition } = await import("/src/lib/offlineUnread.js");
    const me = useAuthStore.getState().authUser?._id, peer = useChatStore.getState().selectedUser?._id;
    return me && peer && Boolean(await readPosition(me, peer));
  });
  await page.goto("about:blank");
  await otherPage.getByLabel("Message", { exact: true }).fill("Arrived while away");
  await otherPage.getByLabel("Message", { exact: true }).press("Enter");
  await otherPage.getByText("Arrived while away", { exact: true }).waitFor();
  await page.goto(frontend);
  await page.getByTitle("Your account", { exact: true }).waitFor();
  await friendEntry.getByLabel("New messages").waitFor();
  await friendEntry.click();
  await page.getByText("Arrived while away", { exact: true }).waitFor();
  assert.equal(await friendEntry.getByLabel("New messages").count(), 0);
  console.log("PASS: direct messages received while offline show an unread indicator after reopening.");
  const directOriginal = page.locator("article.message").filter({ hasText: "A new private hello" });
  await directOriginal.getByRole("button", { name: "Reply to message" }).click();
  await page.getByLabel("Message", { exact: true }).fill("A private reply");
  await page.getByLabel("Message", { exact: true }).press("Enter");
  const directReply = otherPage.locator("article.message").filter({ hasText: "A private reply" });
  await directReply.getByText("Replying to: A new private hello").waitFor();
  const replyRecord = await Message.findOne({ senderId: tester._id, receiverId: (await User.findOne({email: "voice-peer@example.invalid"}))._id }).sort({ createdAt: -1 });
  assert.ok(!JSON.stringify(replyRecord).includes("A private reply"));
  otherPage.once("dialog", dialog => dialog.accept());
  await otherPage.locator("article.message").filter({ hasText: "A new private hello" }).getByRole("button", { name: "Delete message" }).click();
  await directReply.getByText("Replying to: Original message unavailable").waitFor();
  const ownReply = page.locator("article.message").filter({ hasText: "A private reply" });
  await ownReply.getByRole("button", { name: "Add reaction", exact: true }).click();
  await ownReply.getByRole("button", { name: `Add ${"\u{1F44D}"} reaction`, exact: true }).click();
  await directReply.getByRole("button", { name: `Add ${"\u{1F44D}"} reaction`, exact: true }).waitFor();
  await ownReply.getByRole("button", { name: `Remove ${"\u{1F44D}"} reaction`, exact: true }).click();
  await directReply.getByRole("button", { name: `Add ${"\u{1F44D}"} reaction`, exact: true }).waitFor({ state: "hidden" });


  await page.evaluate(() => {
    window.__sonarNotifications = [];
    window.Notification = class {
      static permission = "granted";
      static async requestPermission() { return "granted"; }
      constructor(title, options) { window.__sonarNotifications.push({ title, ...options }); }
    };
  });
  await page.getByTitle("Your account", { exact: true }).click();
  await page.getByRole("button", { name: "Privacy & device", exact: true }).click();
  await page.getByRole("button", { name: "Enable notifications", exact: true }).click();
  await page.getByRole("button", { name: "Close settings", exact: true }).click();
  await page.evaluate(() => { Object.defineProperty(document, "hidden", { configurable: true, value: true }); document.dispatchEvent(new Event("visibilitychange")); });
  await otherPage.getByLabel("Message", { exact: true }).fill("Private notification content");
  await otherPage.getByLabel("Message", { exact: true }).press("Enter");
  await page.waitForFunction(() => window.__sonarNotifications.length === 1);
  assert.deepEqual(await page.evaluate(() => window.__sonarNotifications[0]), { title: "Sonar", body: "You have a new message.", tag: "sonar-new-message" });
  await page.evaluate(() => { delete document.hidden; document.dispatchEvent(new Event("visibilitychange")); });

  await page.getByTitle("Start voice call", { exact: true }).click();
  await otherPage.getByRole("button", { name: "Accept", exact: true }).click();
  await page.getByText("Voice connected", { exact: true }).waitFor();
  await otherPage.getByText("Voice connected", { exact: true }).waitFor();
  const audioReceived = async (target) =>
    target.waitForFunction(async () => {
      for (const connection of window.__sonarTestConnections) {
        const stats = await connection.getStats();
        for (const report of stats.values())
          if (
            report.type === "inbound-rtp" &&
            report.kind === "audio" &&
            report.bytesReceived > 0
          )
            return true;
      }
      return false;
    });
  await audioReceived(page);
  await audioReceived(otherPage);
  await page.getByTitle("Your account", { exact: true }).click();
  await page
    .getByRole("button", { name: "Voice & audio", exact: true })
    .click();
  await page.getByLabel("Output volume", { exact: true }).press("End");
  await page.getByLabel("Output volume", { exact: true }).press("ArrowLeft");
  await page.waitForFunction(() =>
    window.__sonarTestAudioElements.some(
      (element) =>
        element.srcObject &&
        Math.abs(element.volume - 0.99) < 0.001 &&
        !element.paused,
    ),
  );
  await page
    .getByRole("button", { name: "Close settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Mute microphone", exact: true })
    .click();
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks
        .filter((track) => track.readyState === "live")
        .every((track) => !track.enabled),
    ),
    true,
  );
  await page
    .getByRole("button", { name: "Unmute microphone", exact: true })
    .click();
  await page.getByRole("button", { name: "Deafen", exact: true }).click();
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks
        .filter((track) => track.readyState === "live")
        .every((track) => !track.enabled),
    ),
    true,
  );
  await page.getByRole("button", { name: "Undeafen", exact: true }).click();
  await page.getByRole("button", { name: "End call", exact: true }).click();
  await page.locator(".call-banner").waitFor({ state: "hidden" });
  await otherPage.locator(".call-banner").waitFor({ state: "hidden" });
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  assert.equal(
    await otherPage.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  await page.getByTitle("Start voice call", { exact: true }).click();
  await otherPage.getByRole("button", { name: "Accept", exact: true }).click();
  await page.getByText("Voice connected", { exact: true }).waitFor();
  await page.evaluate(() => {
    const track = window.__sonarTestTracks.find((item) => item.readyState === "live");
    if (!track) throw new Error("No active microphone track");
    track.stop();
    track.dispatchEvent(new Event("ended"));
  });
  await page.getByText("Microphone disconnected. Choose another device and call again.", { exact: true }).waitFor();
  await page.locator(".call-banner").waitFor({ state: "hidden" });
  await otherPage.locator(".call-banner").waitFor({ state: "hidden" });
  await page.getByTitle("Start voice call", { exact: true }).click();
  await otherPage
    .getByRole("button", { name: "Decline call", exact: true })
    .click();
  await page.locator(".call-banner").waitFor({ state: "hidden" });
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  await page.getByTitle("Start voice call", { exact: true }).click();
  await otherPage.getByRole("button", { name: "Accept", exact: true }).click();
  await page.getByText("Voice connected", { exact: true }).waitFor();
  await otherContext.close();
  await page.locator(".call-banner").waitFor({ state: "hidden" });
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  console.log(
    "PASS: two-browser voice transport, received audio packets, mute/deafen, and hangup track cleanup.",
  );
  await page.getByTitle("Your account", { exact: true }).click();
  await page
    .getByRole("button", { name: "Voice & audio", exact: true })
    .click();
  const capturedBeforeTest = await page.evaluate(
    () => window.__sonarTestTracks.length,
  );
  await page
    .getByRole("button", { name: "Test speakers", exact: true })
    .click();
  await page.waitForFunction(() =>
    window.__sonarTestAudioElements.some(
      (element) => element.src.startsWith("blob:") && !element.paused,
    ),
  );
  assert.equal(
    await page.evaluate(() => window.__sonarTestTracks.length),
    capturedBeforeTest,
  );
  await page
    .getByRole("button", { name: "Test speakers", exact: true })
    .waitFor();
  await page.getByLabel("Output device", { exact: true }).selectOption("");
  await mkdir(resolve(root, "backend/.local"), { recursive: true });
  await page
    .locator(".speaker-settings")
    .screenshot({ path: resolve(root, "backend/.local/speaker-settings.png") });
  console.log(
    "PASS: active-call output volume, local speaker test playback, and no microphone capture for the tone.",
  );
  await page
    .getByRole("button", { name: "Test microphone", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Stop microphone test", exact: true })
    .waitFor();
  await page.waitForFunction(
    () =>
      Number(
        document.querySelector('[role="meter"]').getAttribute("aria-valuenow"),
      ) > 0,
  );
  await mkdir(resolve(root, "backend/.local"), { recursive: true });
  await page.screenshot({
    path: resolve(root, "backend/.local/voice-settings.png"),
  });
  await page
    .getByRole("button", { name: "Stop microphone test", exact: true })
    .click();
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  await page
    .getByRole("button", { name: "Test microphone", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Stop microphone test", exact: true })
    .waitFor();
  await page
    .getByRole("button", { name: "Close settings", exact: true })
    .click();
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  console.log(
    "PASS: microphone input meter, stop, and settings-close cleanup.",
  );
  const extraSessionContext = await browser.newContext();
  const extraSessionPage = await extraSessionContext.newPage();
  await extraSessionPage.goto(frontend);
  assert.equal(await extraSessionPage.evaluate(async () => (await fetch("/api/auth/login", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: "browser@example.invalid", password: "TenChars!!" }) })).status), 200);
  await page.getByTitle("Your account", { exact: true }).click();
  console.log("Browser: opening settings");
  await page
    .getByRole("button", { name: "Password & passkeys", exact: true })
    .click();
  await page.getByText("Signed-in sessions", { exact: true }).waitFor();
  const remoteSession = page.locator(".session-list li").filter({ hasNotText: "This session" });
  await remoteSession.getByRole("button", { name: "Sign out", exact: true }).waitFor();
  page.once("dialog", dialog => dialog.accept());
  await remoteSession.getByRole("button", { name: "Sign out", exact: true }).click();
  await page.getByText("Session signed out", { exact: true }).waitFor();
  assert.equal(await extraSessionPage.evaluate(async () => (await fetch("/api/auth/check")).status), 401);
  assert.equal(await page.evaluate(async () => (await fetch("/api/auth/check")).status), 200);
  await extraSessionContext.close();
  await page
    .getByRole("button", { name: "Add a passkey", exact: true })
    .click();
  await page
    .getByLabel("Give it a name", { exact: true })
    .fill("My test laptop");
  await page
    .locator('.passkey-setup input[type="password"]')
    .fill("TenChars!!");
  let savedCredential;
  async function prepareAuthenticator(popup, credential) {
    await popup.waitForURL("**/passkey");
    const cdp = await context.newCDPSession(popup);
    await cdp.send("WebAuthn.enable");
    const { authenticatorId } = await cdp.send(
      "WebAuthn.addVirtualAuthenticator",
      {
        options: {
          protocol: "ctap2",
          transport: "internal",
          hasResidentKey: true,
          hasUserVerification: true,
          isUserVerified: true,
          automaticPresenceSimulation: true,
        },
      },
    );
    if (credential)
      await cdp.send("WebAuthn.addCredential", { authenticatorId, credential });
    else
      cdp.on("WebAuthn.credentialAdded", (event) => {
        savedCredential = event.credential;
      });
    await popup.getByRole("button", { name: "Continue with passkey" }).click();
  }
  const registrationPopup = page.waitForEvent("popup");
  console.log("Browser: creating passkey");
  await page
    .getByRole("button", { name: "Create passkey", exact: true })
    .click();
  await prepareAuthenticator(await registrationPopup);
  await page.getByText("My test laptop", { exact: true }).waitFor();
  console.log("Browser: passkey created");
  assert.ok(
    savedCredential,
    "virtual authenticator produced a discoverable credential",
  );
  await mkdir(resolve(root, "backend/.local"), { recursive: true });
  await page.screenshot({
    path: resolve(root, "backend/.local/account-security.png"),
  });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
    "settings fit the mobile viewport",
  );
  await page.screenshot({
    path: resolve(root, "backend/.local/account-security-mobile.png"),
  });
  await page.setViewportSize({ width: 1280, height: 1050 });
  await page
    .getByRole("button", { name: "Sign out everywhere", exact: true })
    .click();
  const loginPopup = page.waitForEvent("popup");
  console.log("Browser: signing in with passkey");
  await page
    .getByRole("button", { name: "Sign in with a passkey", exact: true })
    .click();
  await prepareAuthenticator(await loginPopup, savedCredential);
  await page.getByTitle("Your account", { exact: true }).click();
  await page
    .getByRole("button", { name: "Password & passkeys", exact: true })
    .click();
  await page.getByLabel("Current password", { exact: true }).fill("TenChars!!");
  await page.getByLabel("New password", { exact: true }).fill("NewPass123");
  await page
    .getByLabel("Confirm new password", { exact: true })
    .fill("NewPass123");
  assert.equal(
    await page
      .getByLabel("New password", { exact: true })
      .getAttribute("minlength"),
    "10",
  );
  await page
    .getByRole("button", { name: "Update password & sign out", exact: true })
    .click();
  await page
    .getByLabel("Email address", { exact: true })
    .fill("browser@example.invalid");
  await page.getByLabel("Password", { exact: true }).fill("NewPass123");
  await page
    .getByRole("button", { name: "Enter your space", exact: true })
    .click();
  await page.getByTitle("Your account", { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log(
    "PASS: browser enrollment, localhost popup, passkey login, and 10-character password change.",
  );
  const recoveryContext = await browser.newContext();
  const recoveryPage = await recoveryContext.newPage();
  await recoveryPage.goto(`${frontend}/signup`);
  await recoveryPage.getByLabel("Display name", { exact: true }).fill("Forgetful member");
  await recoveryPage.getByLabel("Email address", { exact: true }).fill("forgetful@example.invalid");
  await recoveryPage.getByLabel("Password", { exact: true }).fill("OriginalPass1");
  await recoveryPage.getByRole("button", { name: "Create your account" }).click();
  const firstRecoveryKey = await saveRecoveryKey(recoveryPage);
  await recoveryPage.getByTitle("Your account", { exact: true }).waitFor();
  const resetContext = await browser.newContext();
  const resetPage = await resetContext.newPage();
  await resetPage.goto(`${frontend}/login`);
  await resetPage.getByRole("link", { name: "Forgot your password?" }).click();
  await resetPage.getByLabel("Email address", { exact: true }).fill("forgetful@example.invalid");
  await resetPage.getByLabel("Recovery key", { exact: true }).fill("0000-0000-0000-0000-0000-0000");
  await resetPage.getByLabel("New password", { exact: true }).fill("ResetPass123");
  await resetPage.getByLabel("Confirm new password", { exact: true }).fill("ResetPass123");
  await resetPage.getByRole("button", { name: "Reset password" }).click();
  await resetPage.getByText("That email and recovery key do not match.").waitFor();
  await resetPage.getByLabel("Recovery key", { exact: true }).fill(firstRecoveryKey.toLowerCase());
  await resetPage.getByRole("button", { name: "Reset password" }).click();
  assert.notEqual(await saveRecoveryKey(resetPage), firstRecoveryKey);
  await resetPage.getByTitle("Your account", { exact: true }).waitFor();
  await recoveryContext.close();
  await resetContext.close();
  console.log("PASS: signup shows a recovery key, and it resets a forgotten password in a new browser.");
  await page.getByTitle("Communities", { exact: true }).click();
  await page.getByTitle("History crew", { exact: true }).click();
  await page.getByTitle("Create a voice room", { exact: true }).click();
  await page.getByLabel("Room name", { exact: true }).fill("lounge");
  await page
    .getByRole("button", { name: "Create voice room", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Join voice room lounge", exact: true })
    .waitFor();
  const inviteResult = await page.evaluate(async (communityId) => {
    const result = await fetch(`/api/communities/${communityId}/invite`, {
      method: "POST",
    });
    return result.json();
  }, String(crew._id));
  const roomBrowsers = [];
  for (let index = 0; index < 2; index++) {
    await User.create({
      fullName: `Room friend ${index + 1}`,
      email: `room-browser-${index}@example.invalid`,
      password: await bcrypt.hash("TenChars!!", 4),
    });
    const roomContext = await browser.newContext({
      permissions: ["microphone"],
      viewport: { width: 1280, height: 1050 },
    });
    const roomPage = await roomContext.newPage();
    await roomPage.addInitScript(instrumentation);
    await roomPage.goto(frontend);
    await roomPage
      .getByLabel("Email address", { exact: true })
      .fill(`room-browser-${index}@example.invalid`);
    await roomPage.getByLabel("Password", { exact: true }).fill("TenChars!!");
    await roomPage
      .getByRole("button", { name: "Enter your space", exact: true })
      .click();
    await saveRecoveryKey(roomPage);
    await roomPage.getByTitle("Your account", { exact: true }).waitFor();
    assert.equal(
      await roomPage.evaluate(async (token) => {
        const result = await fetch("/api/communities/join", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ token }),
        });
        return result.status;
      }, inviteResult.token),
      200,
    );
    await roomPage.getByTitle("Communities", { exact: true }).click();
    await roomPage.getByTitle("History crew", { exact: true }).click();
    roomBrowsers.push({ context: roomContext, page: roomPage });
  }
  await page.getByRole("button", { name: "Back to messages", exact: true }).click();
  await roomBrowsers[0].page.getByLabel("Message general", { exact: true }).fill("Community hello");
  await roomBrowsers[0].page.getByLabel("Message general", { exact: true }).press("Enter");
  await page.getByLabel("New community messages").waitFor();
  await page.getByTitle("Communities", { exact: true }).click();
  await page.getByTitle("History crew", { exact: true }).click();
  await page.getByText("Community hello", { exact: true }).waitFor();
  assert.equal(await page.getByLabel("New community messages").count(), 0);
  await page.waitForFunction(async () => {
    const { useAuthStore } = await import("/src/store/useAuthStore.js");
    const { useCommunityStore } = await import("/src/store/useCommunityStore.js");
    const { readChannelPosition } = await import("/src/lib/offlineUnread.js");
    const me = useAuthStore.getState().authUser?._id, community = useCommunityStore.getState().community?._id, channel = useCommunityStore.getState().channel?._id;
    return me && community && channel && Boolean(await readChannelPosition(me, community, channel));
  });
  await page.goto("about:blank");
  await roomBrowsers[0].page.getByLabel("Message general", { exact: true }).fill("Community message while away");
  await roomBrowsers[0].page.getByLabel("Message general", { exact: true }).press("Enter");
  await roomBrowsers[0].page.getByText("Community message while away", { exact: true }).waitFor();
  await page.goto(frontend);
  await page.getByTitle("Your account", { exact: true }).waitFor();
  await page.getByLabel("New community messages").waitFor();
  await page.getByTitle("Communities", { exact: true }).click();
  await page.getByTitle("History crew", { exact: true }).click();
  await page.getByText("Community message while away", { exact: true }).waitFor();
  assert.equal(await page.getByLabel("New community messages").count(), 0);
  console.log("PASS: community messages received offline show an unread indicator after reopening.");
  await page.locator("article.message").filter({ hasText: "Community hello" }).getByRole("button", { name: "Reply to message" }).click();
  await page.getByLabel("Message general", { exact: true }).fill("Community reply");
  await page.getByLabel("Message general", { exact: true }).press("Enter");
  await roomBrowsers[0].page.locator("article.message").filter({ hasText: "Community reply" }).getByText("Replying to: Community hello").waitFor();
  const channelReply = page.locator("article.message").filter({ hasText: "Community reply" });
  await channelReply.getByRole("button", { name: "Add reaction", exact: true }).click();
  await channelReply.getByRole("button", { name: `Add ${"\u{1F44D}"} reaction`, exact: true }).click();
  await roomBrowsers[0].page.locator("article.message").filter({ hasText: "Community reply" }).getByRole("button", { name: `Add ${"\u{1F44D}"} reaction`, exact: true }).waitFor();


  await page
    .getByRole("button", { name: "Join voice room lounge", exact: true })
    .click();
  await page
    .getByRole("region", { name: "Community voice room", exact: true })
    .getByText("Voice room: lounge", { exact: true })
    .waitFor();
  for (const item of roomBrowsers)
    await item.page
      .getByRole("button", { name: "Join voice room lounge", exact: true })
      .click();
  for (const target of [page, ...roomBrowsers.map((item) => item.page)]) {
    await target.waitForFunction(async () => {
      let received = 0;
      for (const connection of window.__sonarTestConnections.filter(
        (pc) => pc.connectionState === "connected",
      )) {
        const stats = await connection.getStats();
        if (
          [...stats.values()].some(
            (report) =>
              report.type === "inbound-rtp" &&
              report.kind === "audio" &&
              report.bytesReceived > 0,
          )
        )
          received++;
      }
      return received >= 2;
    });
  }
  await page.getByRole("button", { name: "Lock voice room lounge", exact: true }).click();
  await page.getByRole("button", { name: "Unlock voice room lounge", exact: true }).waitFor();
  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Disconnect Room friend 2 from voice", exact: true }).click();
  await roomBrowsers[1].page.getByRole("region", { name: "Community voice room", exact: true }).waitFor({ state: "hidden" });
  assert.equal(await roomBrowsers[1].page.evaluate(() => window.__sonarTestTracks.every(track => track.readyState === "ended")), true);
  await page.getByRole("button", { name: "Unlock voice room lounge", exact: true }).click();
  await roomBrowsers[1].page.getByRole("button", { name: "Join voice room lounge", exact: true }).waitFor({ state: "visible" });
  await roomBrowsers[1].page.getByRole("button", { name: "Join voice room lounge", exact: true }).click();
  await roomBrowsers[1].page.getByRole("region", { name: "Community voice room", exact: true }).waitFor();
  await page
    .getByRole("button", { name: "Mute room microphone", exact: true })
    .click();
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks
        .filter((track) => track.readyState === "live")
        .every((track) => !track.enabled),
    ),
    true,
  );
  await page
    .getByRole("button", { name: "Unmute room microphone", exact: true })
    .click();
  await roomBrowsers[0].page
    .getByRole("button", { name: "Deafen room", exact: true })
    .click();
  assert.equal(
    await roomBrowsers[0].page.evaluate(() =>
      window.__sonarTestAudioElements
        .filter((element) => element.srcObject)
        .every((element) => element.muted),
    ),
    true,
  );
  await roomBrowsers[0].page
    .getByRole("button", { name: "Undeafen room", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Back to messages", exact: true })
    .click();
  await page.getByText("Voice room: lounge", { exact: true }).waitFor();
  await page.getByTitle("Communities", { exact: true }).click();
  await page.getByTitle("History crew", { exact: true }).click();
  await page.screenshot({
    path: resolve(root, "backend/.local/voice-room.png"),
  });
  await page.getByTitle("Members", { exact: true }).click();
  await page.getByLabel("Manage Room friend 1", { exact: true }).selectOption("promote");
  await page.getByRole("button", { name: "Make moderator", exact: true }).click();
  await page.getByText("Moderator added", { exact: true }).waitFor();
  await page.locator(".community-members .person").filter({ hasText: "Room friend 1" }).getByText("Moderator", { exact: true }).waitFor();
  await page.getByLabel("Manage Room friend 1", { exact: true }).selectOption("demote");
  await page.getByRole("button", { name: "Remove moderator role", exact: true }).click();
  await page.getByText("Moderator removed", { exact: true }).waitFor();
  await page
    .getByLabel("Manage Room friend 1", { exact: true })
    .selectOption("ban");
  await page
    .getByRole("button", { name: "Ban member", exact: true })
    .click();
  await roomBrowsers[0].page
    .getByRole("region", { name: "Community voice room", exact: true })
    .waitFor({ state: "hidden" });
  assert.equal(
    await roomBrowsers[0].page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  await page.getByRole("button", { name: "Manage bans", exact: true }).click();
  await page.getByText("Room friend 1", { exact: true }).first().waitFor();
  await page.getByRole("button", { name: "Remove ban", exact: true }).click();
  await page.getByRole("button", { name: "Remove ban", exact: true }).click();
  await page.getByText("Ban removed. Create a new invite to let them rejoin.", { exact: true }).waitFor();
  await roomBrowsers[1].page.evaluate(() => {
    const track = window.__sonarTestTracks.find(item => item.readyState === "live");
    if (!track) throw new Error("No active room microphone track");
    track.stop();
    track.dispatchEvent(new Event("ended"));
  });
  await roomBrowsers[1].page.getByText("Microphone disconnected. Choose another device and rejoin the room.", { exact: true }).waitFor();
  await roomBrowsers[1].page
    .getByRole("region", { name: "Community voice room", exact: true })
    .waitFor({ state: "hidden" });
  await page
    .getByRole("button", { name: "Leave voice room", exact: true })
    .click();
  await page
    .getByRole("region", { name: "Community voice room", exact: true })
    .waitFor({ state: "hidden" });
  assert.equal(
    await page.evaluate(() =>
      window.__sonarTestTracks.every((track) => track.readyState === "ended"),
    ),
    true,
  );
  await page.getByRole("button", { name: "Delete community", exact: true }).click();
  await page.getByRole("button", { name: "Delete community permanently", exact: true }).click();
  await page.getByText("Community deleted", { exact: true }).waitFor();
  await roomBrowsers[1].page.getByText("This community was deleted.", { exact: true }).waitFor();
  assert.equal(await Community.exists({ _id: crew._id }), null);
  assert.equal(await ChannelMessage.countDocuments({ communityId: crew._id }), 0);
  for (const item of roomBrowsers) await item.context.close();
  console.log(
    "PASS: three-browser room audio between every pair, mute/deafen, navigation, member removal, and leave cleanup.",
  );
  console.log("Screenshot: backend/.local/account-security.png");
} finally {
  await browser?.close();
  await vite?.close();
  await new Promise((resolve) => io.close(resolve));
  await mongoose.disconnect();
  await database.stop();
}
