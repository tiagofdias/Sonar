// Watch Together: friends in a voice room or an answered call watch YouTube in sync.
// Only the queue and playback position live here, in memory. Video plays from
// YouTube in each person's own browser; nothing about it passes through Sonar.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import User from "../models/user.model.js";
import { voiceRoomUsers, answeredCalls } from "./voiceActivity.js";

export const MAX_QUEUE = 100;
export const PLAYLIST_LIMIT = 50;
export const REACTIONS = ["😂", "😮", "❤️", "👏", "🔥", "😢", "👀", "🎉"];
const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;
const LIST_ID = /^[A-Za-z0-9_-]{10,64}$/;
const YOUTUBE_HOSTS = new Set(["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "youtu.be", "www.youtube-nocookie.com", "youtube-nocookie.com"]);
// Sponsor reads, self-promotion and "subscribe" reminders. Intros and outros stay.
const SKIP_CATEGORIES = ["sponsor", "selfpromo", "interaction"];

const sessions = new Map();
let io = null;
let fetcher = (url) => fetch(url, { signal: AbortSignal.timeout(4000), headers: { "User-Agent": "Sonar watch-together" } });
/** Tests replace network lookups (oEmbed, SponsorBlock, playlists). */
export function setWatchFetch(next) { fetcher = next; }
export function resetWatch() { for (const session of sessions.values()) clearTimeout(session.skipTimer); sessions.clear(); links.clear(); infoCache.clear(); segmentCache.clear(); }

/** The YouTube video or playlist a pasted link points at, or null. */
export function parseYouTube(text) {
  let url;
  try { url = new URL(String(text).trim()); } catch { return null; }
  if (!["https:", "http:"].includes(url.protocol) || !YOUTUBE_HOSTS.has(url.hostname.toLowerCase())) return null;
  const host = url.hostname.toLowerCase();
  const parts = url.pathname.split("/").filter(Boolean);
  let videoId = null;
  if (host === "youtu.be") videoId = parts[0];
  else if (parts[0] === "watch") videoId = url.searchParams.get("v");
  else if (["shorts", "embed", "live", "v"].includes(parts[0])) videoId = parts[1];
  const list = url.searchParams.get("list");
  const start = Number.parseInt(url.searchParams.get("t") || url.searchParams.get("start") || "0", 10);
  const result = {
    videoId: VIDEO_ID.test(videoId || "") ? videoId : null,
    list: LIST_ID.test(list || "") ? list : null,
    start: Number.isFinite(start) && start > 0 && start < 86400 ? start : 0,
  };
  return result.videoId || result.list ? result : null;
}

// ----- Who belongs to which watch scope ------------------------------------

/** A person's current call or voice room, which is where they can watch. */
export function scopeOf(userId) {
  const id = String(userId);
  const room = voiceRoomUsers.get(id);
  if (room) return `room:${room.key}`;
  for (const [callId, people] of answeredCalls) if (people.includes(id)) return `call:${callId}`;
  return null;
}
function membersOf(scope) {
  if (scope.startsWith("room:")) {
    const key = scope.slice(5);
    return [...voiceRoomUsers].filter(([, active]) => active.key === key).map(([id]) => id);
  }
  return [...(answeredCalls.get(scope.slice(5)) || [])];
}

// ----- Sessions --------------------------------------------------------------

const now = () => Date.now();
function positionOf(session, at = now()) {
  if (!session.playing || session.hold || session.current < 0) return session.position;
  return session.position + (at - session.anchorAt) / 1000;
}
function setPosition(session, position, at = now()) {
  session.position = Math.max(0, Math.min(86400, position));
  session.anchorAt = at;
}
function waitingFor(session) {
  return [...session.viewers].filter(([, viewer]) => viewer.state === "waiting" && !viewer.skipped).map(([id]) => id);
}
export function snapshot(session) {
  const item = session.queue[session.current];
  return {
    scope: session.scope,
    queue: session.queue.map(({ id, videoId, title, thumb, addedBy, addedByName }) => ({ id, videoId, title, thumb, addedBy, addedByName })),
    current: session.current,
    playing: session.playing,
    position: positionOf(session),
    serverNow: now(),
    hold: session.hold ? { since: session.hold.since, waitingFor: waitingFor(session) } : null,
    viewers: [...session.viewers].map(([userId, viewer]) => ({ userId, name: viewer.name, animal: viewer.animal, state: viewer.skipped ? "catching-up" : viewer.state })),
    segments: item ? segmentCache.get(item.videoId) || [] : [],
    playlists: Boolean(process.env.YOUTUBE_API_KEY),
  };
}
function announce(session) {
  if (!io) return;
  io.to(`watch:${session.scope}`).emit("watch:state", snapshot(session));
  // People in the call who are not watching yet see that a session is running.
  const item = session.queue[session.current];
  const note = { scope: session.scope, active: true, title: item?.title || null, viewers: session.viewers.size, startedBy: session.startedByName };
  for (const member of membersOf(session.scope)) io.to(member).emit("watch:available", note);
  scheduleSkip(session);
}
function end(session) {
  clearTimeout(session.skipTimer);
  sessions.delete(session.scope);
  if (!io) return;
  io.to(`watch:${session.scope}`).emit("watch:ended", { scope: session.scope });
  io.in(`watch:${session.scope}`).socketsLeave(`watch:${session.scope}`);
  for (const member of membersOf(session.scope)) io.to(member).emit("watch:available", { scope: session.scope, active: false });
}

/** Drops viewers who left the call or room; ends sessions nobody can watch any more. */
export function watchMembersChanged() {
  for (const session of [...sessions.values()]) {
    const members = new Set(membersOf(session.scope));
    let changed = false;
    for (const id of [...session.viewers.keys()]) if (!members.has(id)) { session.viewers.delete(id); changed = true; }
    if (!members.size || !session.viewers.size) end(session);
    else if (changed) { releaseIfReady(session); announce(session); }
  }
}

function releaseIfReady(session) {
  if (session.hold && !waitingFor(session).length) {
    session.hold = null;
    setPosition(session, session.position);
  }
}

// When playback reaches a SponsorBlock segment, everyone jumps past it together.
function scheduleSkip(session) {
  clearTimeout(session.skipTimer);
  const item = session.queue[session.current];
  if (!item || !session.playing || session.hold) return;
  const position = positionOf(session);
  const next = (segmentCache.get(item.videoId) || []).find(([start, finish]) => finish > position + 0.5 && start >= position - 0.5);
  const inside = (segmentCache.get(item.videoId) || []).find(([start, finish]) => start <= position && finish > position + 0.5);
  const target = inside || next;
  if (!target) return;
  const delay = inside ? 0 : (target[0] - position) * 1000;
  session.skipTimer = setTimeout(() => {
    if (sessions.get(session.scope) !== session || session.queue[session.current] !== item || !session.playing || session.hold) return;
    setPosition(session, target[1]);
    if (io) io.to(`watch:${session.scope}`).emit("watch:skipped", { reason: "sponsor" });
    announce(session);
  }, Math.max(0, Math.min(delay, 6 * 3600 * 1000)));
}

// ----- Video details (fetched by the server so browsers only talk to YouTube's player) ----

const infoCache = new Map();
const segmentCache = new Map();
const remember = (cache, key, value, limit = 1000) => { if (cache.size >= limit) cache.delete(cache.keys().next().value); cache.set(key, value); return value; };
const thumbOf = (videoId) => `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg`;

async function videoInfo(videoId) {
  if (infoCache.has(videoId)) return infoCache.get(videoId);
  let title = "YouTube video";
  try {
    const response = await fetcher(`https://www.youtube.com/oembed?format=json&url=${encodeURIComponent(`https://www.youtube.com/watch?v=${videoId}`)}`);
    if (response.ok) {
      const data = await response.json();
      if (typeof data?.title === "string" && data.title.trim()) title = data.title.trim().slice(0, 200);
    }
  } catch { /* The plain title is fine. */ }
  return remember(infoCache, videoId, { title, thumb: thumbOf(videoId) });
}

/** SponsorBlock segments, looked up by a short hash prefix so the service cannot tell which video. */
export async function sponsorSegments(videoId) {
  if (segmentCache.has(videoId)) return segmentCache.get(videoId);
  let segments = [];
  try {
    const prefix = createHash("sha256").update(videoId).digest("hex").slice(0, 4);
    const response = await fetcher(`https://sponsor.ajay.app/api/skipSegments/${prefix}?categories=${encodeURIComponent(JSON.stringify(SKIP_CATEGORIES))}`);
    if (response.ok) {
      const rows = await response.json();
      const row = Array.isArray(rows) ? rows.find((entry) => entry?.videoID === videoId) : null;
      segments = (row?.segments || [])
        .filter((entry) => entry?.actionType === "skip" && Array.isArray(entry.segment))
        .map(({ segment: [start, finish] }) => [Number(start), Number(finish)])
        .filter(([start, finish]) => Number.isFinite(start) && Number.isFinite(finish) && start >= 0 && finish - start >= 1 && finish < 86400)
        .sort((a, b) => a[0] - b[0])
        .slice(0, 50);
    }
  } catch { segments = []; }
  return remember(segmentCache, videoId, segments);
}

async function playlistVideos(list) {
  const key = process.env.YOUTUBE_API_KEY;
  if (!key) return null;
  try {
    const response = await fetcher(`https://www.googleapis.com/youtube/v3/playlistItems?part=snippet&maxResults=${PLAYLIST_LIMIT}&playlistId=${encodeURIComponent(list)}&key=${encodeURIComponent(key)}`);
    if (!response.ok) return [];
    const data = await response.json();
    return (data?.items || []).flatMap((entry) => {
      const videoId = entry?.snippet?.resourceId?.videoId;
      const title = entry?.snippet?.title;
      if (!VIDEO_ID.test(videoId || "") || title === "Private video" || title === "Deleted video") return [];
      remember(infoCache, videoId, { title: String(title || "YouTube video").slice(0, 200), thumb: thumbOf(videoId) });
      return [videoId];
    });
  } catch { return []; }
}

// ----- One-time links that let the desktop app hand watching to a browser ----

const links = new Map();
const LINK_MS = 2 * 60 * 1000;
const tokenHash = (token) => createHash("sha256").update(token).digest("hex");
export function createWatchLink(userId, sessionVersion) {
  const scope = scopeOf(userId);
  if (!scope) return null;
  const at = now();
  for (const [key, link] of links) if (link.expiresAt <= at) links.delete(key);
  if (links.size > 5000) return null;
  const token = randomBytes(32).toString("base64url");
  links.set(tokenHash(token), { userId: String(userId), scope, sessionVersion, expiresAt: at + LINK_MS });
  return { token, scope };
}
/** Single use: a link works once, for two minutes. */
export function redeemWatchLink(token) {
  if (typeof token !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(token)) return null;
  const key = tokenHash(token);
  const link = links.get(key);
  links.delete(key);
  return link && link.expiresAt > now() ? link : null;
}

// ----- Socket events -----------------------------------------------------------

export function installWatch(server, { onChange = () => {} } = {}) {
  io = server;
  return (socket) => {
    let count = 0;
    const resetCount = setInterval(() => { count = 0; }, 10000);
    let lastReaction = 0;
    const scope = () => {
      const current = scopeOf(socket.userId);
      // A browser opened from the desktop app may only watch the call it was opened for.
      if (socket.watchOnly && current !== socket.watchScope) return null;
      return current;
    };
    const handle = (event, action) => socket.on(event, async (payload = {}, ack) => {
      const reply = (value) => { if (typeof ack === "function") ack(value); };
      if (++count > 300) return reply({ error: "Slow down a little." });
      try {
        const where = scope();
        if (!where) return reply({ error: "Join a call or voice room to watch together." });
        await action(payload || {}, reply, where);
      } catch {
        reply({ error: "Watch together had a problem. Try again." });
      }
    });
    const viewing = (where, reply) => {
      const session = sessions.get(where);
      if (!session?.viewers.has(socket.userId)) { reply({ error: "Join watch together first." }); return null; }
      return session;
    };
    const changed = (session, reply) => { announce(session); onChange(); reply({ ok: true, state: snapshot(session) }); };
    const addVideos = async (session, ids, start, name) => {
      const room = MAX_QUEUE - session.queue.length;
      const chosen = ids.slice(0, Math.max(0, room));
      const details = await Promise.all(chosen.map(videoInfo));
      chosen.forEach((videoId, index) => session.queue.push({ id: randomUUID(), videoId, ...details[index], addedBy: socket.userId, addedByName: name }));
      // Segments are looked up in the background; skips start once they arrive.
      for (const videoId of chosen) if (!segmentCache.has(videoId)) sponsorSegments(videoId).then(() => { if (sessions.get(session.scope) === session) scheduleSkip(session); });
      if (session.current < 0 && session.queue.length) {
        session.current = 0;
        session.playing = true;
        setPosition(session, start);
      }
      return chosen.length;
    };

    handle("watch:join", async (_payload, reply, where) => {
      const user = await User.findById(socket.userId).select("fullName animal").lean();
      let session = sessions.get(where);
      if (!session) {
        session = { scope: where, queue: [], current: -1, playing: false, position: 0, anchorAt: now(), hold: null, viewers: new Map(), startedByName: user?.fullName || "Someone" };
        sessions.set(where, session);
      }
      const existing = session.viewers.get(socket.userId);
      session.viewers.set(socket.userId, { name: user?.fullName || "Someone", animal: user?.animal || null, state: existing?.state || "joining", skipped: false, socketId: socket.id });
      socket.join(`watch:${where}`);
      changed(session, reply);
    });
    handle("watch:leave", async (_payload, reply, where) => {
      const session = sessions.get(where);
      socket.leave(`watch:${where}`);
      if (!session) return reply({ ok: true });
      session.viewers.delete(socket.userId);
      if (!session.viewers.size) { end(session); return reply({ ok: true }); }
      releaseIfReady(session);
      changed(session, reply);
    });
    handle("watch:state", async (_payload, reply, where) => {
      const session = sessions.get(where);
      reply({ ok: true, state: session ? snapshot(session) : null });
    });
    handle("watch:add", async ({ url }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      const parsed = parseYouTube(url);
      if (!parsed) return reply({ error: "Paste a YouTube link." });
      if (session.queue.length >= MAX_QUEUE) return reply({ error: `The queue holds ${MAX_QUEUE} videos.` });
      const name = session.viewers.get(socket.userId).name;
      let ids = parsed.videoId ? [parsed.videoId] : [];
      if (parsed.list && !parsed.videoId) {
        const listed = await playlistVideos(parsed.list);
        if (listed === null) return reply({ error: "Playlists need a YouTube API key on this server. Add videos one at a time." });
        if (!listed.length) return reply({ error: "That playlist is empty or private." });
        ids = listed;
      }
      if (sessions.get(where) !== session) return reply({ error: "Watch together ended." });
      const added = await addVideos(session, ids, parsed.start, name);
      changed(session, (value) => reply({ ...value, added }));
    });
    handle("watch:remove", async ({ id }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      const index = session.queue.findIndex((item) => item.id === id);
      if (index < 0) return reply({ error: "That video is no longer in the queue." });
      session.queue.splice(index, 1);
      if (index < session.current) session.current -= 1;
      else if (index === session.current) {
        // The next video starts; if this was the last one, playback stops.
        if (session.current >= session.queue.length) { session.current = session.queue.length - 1; session.playing = false; }
        session.hold = null;
        setPosition(session, 0);
        for (const viewer of session.viewers.values()) viewer.skipped = false;
      }
      changed(session, reply);
    });
    handle("watch:move", async ({ id, to }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      const from = session.queue.findIndex((item) => item.id === id);
      if (from < 0 || !Number.isInteger(to) || to < 0 || to >= session.queue.length) return reply({ error: "That move is not possible." });
      const playing = session.queue[session.current];
      const [item] = session.queue.splice(from, 1);
      session.queue.splice(to, 0, item);
      session.current = playing ? session.queue.indexOf(playing) : session.current;
      changed(session, reply);
    });
    const advance = (session, to) => {
      if (to >= 0 && to < session.queue.length) {
        session.current = to;
        session.playing = true;
      } else session.playing = false;
      session.hold = null;
      for (const viewer of session.viewers.values()) viewer.skipped = false;
      setPosition(session, 0);
    };
    handle("watch:skip", async (_payload, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      advance(session, session.current + 1);
      changed(session, reply);
    });
    handle("watch:play-item", async ({ id }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      const index = session.queue.findIndex((item) => item.id === id);
      if (index < 0) return reply({ error: "That video is no longer in the queue." });
      advance(session, index);
      changed(session, reply);
    });
    // Sent by viewers when their video reaches the end; only the first report counts.
    handle("watch:ended", async ({ id }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (session.queue[session.current]?.id !== id) return reply({ ok: true });
      advance(session, session.current + 1);
      changed(session, reply);
    });
    handle("watch:play", async (_payload, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (session.current < 0) return reply({ error: "Add a video first." });
      if (!session.playing) { session.playing = true; setPosition(session, session.position); }
      changed(session, reply);
    });
    handle("watch:pause", async (_payload, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (session.playing) { setPosition(session, positionOf(session)); session.playing = false; session.hold = null; }
      changed(session, reply);
    });
    handle("watch:seek", async ({ position }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (typeof position !== "number" || !Number.isFinite(position) || position < 0 || position > 86400) return reply({ error: "Invalid position." });
      setPosition(session, position);
      changed(session, reply);
    });
    // "waiting" while an ad or buffering holds this person's video; "watching" once it plays.
    handle("watch:status", async ({ state }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (!["watching", "waiting"].includes(state)) return reply({ error: "Invalid state." });
      const viewer = session.viewers.get(socket.userId);
      const before = viewer.state;
      viewer.state = state;
      if (state === "watching") viewer.skipped = false;
      if (state === "waiting" && !viewer.skipped && session.playing && !session.hold && session.current >= 0) {
        setPosition(session, positionOf(session));
        session.hold = { since: now() };
      }
      releaseIfReady(session);
      if (before !== state || session.hold) changed(session, reply);
      else reply({ ok: true });
    });
    // After a long wait, the group can carry on; whoever was waiting catches up afterwards.
    handle("watch:continue", async (_payload, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      for (const id of waitingFor(session)) session.viewers.get(id).skipped = true;
      releaseIfReady(session);
      changed(session, reply);
    });
    handle("watch:react", async ({ emoji }, reply, where) => {
      const session = viewing(where, reply);
      if (!session) return;
      if (!REACTIONS.includes(emoji)) return reply({ error: "Choose one of the reactions." });
      if (now() - lastReaction < 500) return reply({ ok: true });
      lastReaction = now();
      io.to(`watch:${where}`).emit("watch:react", { userId: socket.userId, emoji, id: randomUUID() });
      reply({ ok: true });
    });
    socket.on("disconnect", () => {
      clearInterval(resetCount);
      for (const session of [...sessions.values()]) {
        if (session.viewers.get(socket.userId)?.socketId !== socket.id) continue;
        session.viewers.delete(socket.userId);
        if (!session.viewers.size) end(session);
        else { releaseIfReady(session); announce(session); }
      }
    });
  };
}
