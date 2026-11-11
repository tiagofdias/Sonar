import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import jwt from "jsonwebtoken";
import { MongoMemoryServer } from "mongodb-memory-server";
import { io as client } from "socket.io-client";
process.env.JWT_SECRET ||= "test-secret-test-secret-test-secret-1";
const { app, server } = await import("./socket.js");
const { default: User } = await import("../models/user.model.js");
const { default: watchRoutes } = await import("../routes/watch.route.js");
const { voiceRoomUsers, answeredCalls } = await import("./voiceActivity.js");
const { setWatchFetch, watchMembersChanged, parseYouTube, resetWatch } = await import("./watch.js");

const A = "aaaaaaaaaaa", B = "bbbbbbbbbbb", C = "ccccccccccc";
const json = (body) => ({ ok: true, json: async () => body });

test("links are only accepted from YouTube", () => {
  assert.deepEqual(parseYouTube("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42"), { videoId: "dQw4w9WgXcQ", list: null, start: 42 });
  assert.equal(parseYouTube("https://youtu.be/dQw4w9WgXcQ").videoId, "dQw4w9WgXcQ");
  assert.equal(parseYouTube("https://m.youtube.com/shorts/dQw4w9WgXcQ").videoId, "dQw4w9WgXcQ");
  assert.equal(parseYouTube("https://www.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG").list, "PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG");
  for (const bad of ["https://evil.example/watch?v=dQw4w9WgXcQ", "https://youtube.com.evil.example/watch?v=dQw4w9WgXcQ", "javascript:alert(1)", "https://www.youtube.com/watch?v=short", "not a link"])
    assert.equal(parseYouTube(bad), null, bad);
});

test("friends in a voice room watch in sync, wait for each other and share a queue", async (t) => {
  const database = await MongoMemoryServer.create();
  const sockets = [];
  setWatchFetch(async (url) => {
    if (url.includes("oembed")) return json({ title: url.includes(A) ? "Video A" : url.includes(B) ? "Video B" : "Video C" });
    if (url.includes("sponsor.ajay.app")) return json([{ videoID: A, segments: [{ segment: [0.6, 30], actionType: "skip", category: "sponsor" }] }]);
    throw new Error(`unexpected fetch ${url}`);
  });
  try {
    await mongoose.connect(database.getUri("sonar-watch-tests"));
    const [ana, bo, cy] = await User.create(["ana", "bo", "cy"].map((name) => ({ fullName: name, username: name, email: `${name}@example.invalid`, password: "unused-test-hash", animal: "whale" })));
    app.use(express.json());
    app.use(cookieParser());
    app.use("/api/watch", watchRoutes);
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    const cookieFor = (user) => `jwt=${jwt.sign({ userId: String(user._id) }, process.env.JWT_SECRET, { expiresIn: "1h" })}`;
    const connect = async (cookie, auth) => {
      const socket = client(url, { transports: ["websocket"], reconnection: false, autoConnect: false, extraHeaders: { Cookie: cookie }, auth });
      sockets.push(socket);
      socket.states = [];
      socket.on("watch:state", (state) => socket.states.push(state));
      await new Promise((resolve, reject) => { socket.once("connect", resolve); socket.once("connect_error", reject); socket.connect(); });
      return socket;
    };
    const send = (socket, event, payload = {}) => new Promise((resolve) => socket.timeout(3000).emit(event, payload, (error, result) => resolve(error ? { error: "no reply" } : result)));
    const room = "c1:ch1";
    voiceRoomUsers.set(String(ana._id), { key: room });
    voiceRoomUsers.set(String(bo._id), { key: room });
    const a = await connect(cookieFor(ana)), b = await connect(cookieFor(bo)), c = await connect(cookieFor(cy));

    await t.test("only people in the call or room can watch", async () => {
      assert.match((await send(c, "watch:join")).error, /Join a call or voice room/);
      assert.equal((await send(a, "watch:add", { url: "https://youtu.be/" + A })).error, "Join watch together first.");
    });
    await t.test("joining and adding a video starts it for everyone", async () => {
      assert.equal((await send(a, "watch:join")).ok, true);
      assert.equal((await send(b, "watch:join")).ok, true);
      const added = await send(a, "watch:add", { url: "https://www.youtube.com/watch?v=" + A });
      assert.equal(added.added, 1);
      assert.equal(added.state.current, 0);
      assert.equal(added.state.playing, true);
      assert.equal(added.state.queue[0].title, "Video A");
      assert.equal(added.state.queue[0].thumb, `https://i.ytimg.com/vi/${A}/mqdefault.jpg`);
      assert.equal((await send(a, "watch:add", { url: "https://evil.example/watch?v=" + B })).error, "Paste a YouTube link.");
    });
    await t.test("sponsor segments are skipped for everyone at once", async () => {
      await new Promise((resolve) => setTimeout(resolve, 900));
      const { state } = await send(a, "watch:state");
      assert.ok(state.position >= 30, `expected a jump past the segment, got ${state.position}`);
      assert.deepEqual(state.segments, [[0.6, 30]]);
    });
    await t.test("someone held by an ad or buffering makes everyone wait", async () => {
      await send(a, "watch:status", { state: "watching" });
      const held = await send(b, "watch:status", { state: "waiting" });
      assert.deepEqual(held.state.hold.waitingFor, [String(bo._id)]);
      const frozen = held.state.position;
      await new Promise((resolve) => setTimeout(resolve, 300));
      assert.equal((await send(a, "watch:state")).state.position, frozen, "the position does not move while waiting");
      const released = await send(b, "watch:status", { state: "watching" });
      assert.equal(released.state.hold, null);
      assert.ok(Math.abs(released.state.position - frozen) < 0.2, "everyone carries on from where they stopped");
    });
    await t.test("after a long wait the group can continue without them", async () => {
      await send(b, "watch:status", { state: "waiting" });
      const continued = await send(a, "watch:continue");
      assert.equal(continued.state.hold, null);
      assert.equal(continued.state.viewers.find((v) => v.userId === String(bo._id)).state, "catching-up");
      await send(b, "watch:status", { state: "waiting" });
      assert.equal((await send(a, "watch:state")).state.hold, null, "a skipped viewer does not hold everyone again");
      await send(b, "watch:status", { state: "watching" });
    });
    await t.test("anyone can pause, seek and play for everyone", async () => {
      const paused = await send(b, "watch:pause");
      assert.equal(paused.state.playing, false);
      const seeked = await send(a, "watch:seek", { position: 90 });
      assert.equal(seeked.state.position, 90);
      assert.equal((await send(a, "watch:seek", { position: -5 })).error, "Invalid position.");
      const playing = await send(b, "watch:play");
      assert.equal(playing.state.playing, true);
      assert.ok(b.states.length && a.states.at(-1).playing === true, "both received the update");
    });
    await t.test("the queue can be added to, reordered, trimmed and skipped", async () => {
      await send(b, "watch:add", { url: "https://youtu.be/" + B });
      const third = await send(a, "watch:add", { url: "https://youtu.be/" + C });
      const [first, second, last] = third.state.queue;
      assert.deepEqual([first.title, second.title, last.title], ["Video A", "Video B", "Video C"]);
      assert.equal(second.addedByName, "bo");
      const moved = await send(b, "watch:move", { id: last.id, to: 1 });
      assert.deepEqual(moved.state.queue.map((item) => item.title), ["Video A", "Video C", "Video B"]);
      assert.equal(moved.state.current, 0, "the playing video stays current");
      const removed = await send(a, "watch:remove", { id: second.id });
      assert.deepEqual(removed.state.queue.map((item) => item.title), ["Video A", "Video C"]);
      const ended = await send(a, "watch:ended", { id: first.id });
      assert.equal(ended.state.current, 1);
      assert.equal((await send(b, "watch:ended", { id: first.id })).state, undefined, "a second end report for the same video is ignored");
      const skipped = await send(b, "watch:skip");
      assert.equal(skipped.state.playing, false, "skipping the last video stops playback");
    });
    await t.test("reactions reach everyone watching, from a fixed set", async () => {
      const seen = new Promise((resolve) => b.once("watch:react", resolve));
      assert.equal((await send(a, "watch:react", { emoji: "🔥" })).ok, true);
      assert.equal((await seen).emoji, "🔥");
      assert.equal((await send(a, "watch:react", { emoji: "<img>" })).error, "Choose one of the reactions.");
    });
    await t.test("a one-time link opens a browser tab that can only watch", async () => {
      const post = (path, body, cookie) => fetch(url + path, { method: "POST", headers: { "Content-Type": "application/json", ...(cookie ? { Cookie: cookie } : {}) }, body: JSON.stringify(body) });
      assert.equal((await post("/api/watch/link", {}, cookieFor(cy))).status, 409, "only from inside a call or room");
      const { path } = await (await post("/api/watch/link", {}, cookieFor(ana))).json();
      const token = path.split("#t=")[1];
      const redeemed = await post("/api/watch/redeem", { token });
      assert.equal(redeemed.status, 200);
      const watchCookie = redeemed.headers.get("set-cookie").split(";")[0];
      assert.match(watchCookie, /^sonar_watch=/);
      assert.equal((await post("/api/watch/redeem", { token })).status, 410, "a link works once");
      const tab = await connect(watchCookie, { watch: true });
      assert.equal((await send(tab, "watch:join")).ok, true);
      assert.equal((await send(tab, "call:signal", { to: String(bo._id), signal: {} })).error, "no reply", "other events are refused");
      const onlineSeen = new Promise((resolve) => tab.once("getOnlineUsers", () => resolve(true)));
      assert.equal(await Promise.race([onlineSeen, new Promise((resolve) => setTimeout(() => resolve(false), 300))]), false, "presence never reaches it");
      const original = Date.now;
      try {
        const late = await (await post("/api/watch/link", {}, cookieFor(ana))).json();
        Date.now = () => original() + 3 * 60 * 1000;
        assert.equal((await post("/api/watch/redeem", { token: late.path.split("#t=")[1] })).status, 410, "links expire after two minutes");
      } finally { Date.now = original; }
      voiceRoomUsers.delete(String(ana._id));
      watchMembersChanged();
      assert.match((await send(tab, "watch:state")).error, /Join a call or voice room/, "the tab stops working when its owner leaves the room");
      voiceRoomUsers.set(String(ana._id), { key: room });
    });
    await t.test("leaving the room removes a viewer, and the last one out ends the session", async () => {
      await send(a, "watch:join");
      await send(b, "watch:join");
      const ended = new Promise((resolve) => a.once("watch:ended", resolve));
      voiceRoomUsers.delete(String(bo._id));
      watchMembersChanged();
      assert.deepEqual((await send(a, "watch:state")).state.viewers.map((v) => v.name).sort(), ["ana"]);
      voiceRoomUsers.delete(String(ana._id));
      watchMembersChanged();
      await ended;
    });
    await t.test("an answered call is a watch scope for its two people", async () => {
      answeredCalls.set("call-1", [String(ana._id), String(bo._id)]);
      assert.equal((await send(a, "watch:join")).state.scope, "call:call-1");
      assert.match((await send(c, "watch:join")).error, /Join a call or voice room/);
      answeredCalls.delete("call-1");
      watchMembersChanged();
    });
  } finally {
    for (const socket of sockets) socket.disconnect();
    resetWatch();
    voiceRoomUsers.clear();
    answeredCalls.clear();
    await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
