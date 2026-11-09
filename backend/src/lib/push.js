import * as webpush from "web-push";
import PushSubscription, { PushConfig } from "../models/pushSubscription.model.js";
import User from "../models/user.model.js";

// socket.js fills this in, which avoids importing it from here.
export const connection = { isOnline: () => false };

const ALLOWED_HOSTS = [/^fcm\.googleapis\.com$/, /(^|\.)push\.services\.mozilla\.com$/, /(^|\.)notify\.windows\.com$/, /(^|\.)push\.apple\.com$/];
export function allowedEndpoint(value) {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && ALLOWED_HOSTS.some((host) => host.test(url.hostname));
  } catch { return false; }
}

let keys = null;
export async function pushKeys() {
  if (keys) return keys;
  if (process.env.VAPID_PUBLIC_KEY && process.env.VAPID_PRIVATE_KEY) {
    keys = { publicKey: process.env.VAPID_PUBLIC_KEY, privateKey: process.env.VAPID_PRIVATE_KEY };
  } else {
    // Stored so alerts work without extra hosting setup and survive restarts.
    const made = webpush.generateVAPIDKeys();
    keys = await PushConfig.findOneAndUpdate({ _id: "vapid" }, { $setOnInsert: made }, { upsert: true, new: true }).lean();
  }
  webpush.setVapidDetails(process.env.FRONTEND_URL?.startsWith("https://") ? process.env.FRONTEND_URL : "https://sonar.invalid", keys.publicKey, keys.privateKey);
  return keys;
}

const GAP_MS = 30000;
const lastSent = new Map();

// Tells someone with no open Sonar that something arrived, and nothing more.
export async function pushTo(userId, kind, { community, sender } = {}) {
  const id = String(userId);
  try {
    if (connection.isOnline(id)) return;
    const now = Date.now();
    if (kind === "message" && now - (lastSent.get(id) || 0) < GAP_MS) return;
    if (community || sender) {
      const user = await User.findById(id).select("+preferences +blocked").lean();
      if (community && user?.preferences?.mutedCommunities?.some((item) => String(item) === String(community))) return;
      if (sender && user?.blocked?.some((item) => String(item) === String(sender))) return;
    }
    const subscriptions = await PushSubscription.find({ userId: id }).limit(10).lean();
    if (!subscriptions.length) return;
    lastSent.set(id, now);
    if (lastSent.size > 5000) lastSent.delete(lastSent.keys().next().value);
    await pushKeys();
    const body = JSON.stringify({ title: "Sonar", body: kind === "call" ? "Incoming call" : "New message" });
    await Promise.all(subscriptions.map(({ endpoint, keys: pair }) =>
      webpush.sendNotification({ endpoint, keys: pair }, body, { TTL: kind === "call" ? 60 : 3600 }).catch((error) => {
        if ([404, 410].includes(error.statusCode)) return PushSubscription.deleteOne({ endpoint });
      })));
  } catch { /* An alert must never disturb sending. */ }
}
