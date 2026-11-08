import express from "express";
import { createHmac, randomBytes } from "node:crypto";
import { protectRoute } from "../middleware/auth.middleware.js";
export function voiceConfiguration(env = process.env, now = Date.now()) {
  const urls = (value, pattern) => {
    const list = (value || "")
      .split(",")
      .map((url) => url.trim())
      .filter(Boolean);
    if (list.length > 12 || list.some((url) => !pattern.test(url)))
      throw new Error("Invalid ICE configuration");
    return list;
  };
  const stun = urls(env.STUN_URLS, /^stuns?:[^\s]+$/);
  const turn = urls(env.TURN_URLS, /^turns?:[^\s]+$/);
  const iceServers = stun.length ? [{ urls: stun }] : [];
  let expiresAt = null;
  if (turn.length) {
    if (!env.TURN_SHARED_SECRET)
      throw new Error("TURN_SHARED_SECRET is required");
    const expiry = Math.floor(now / 1000) + 3600;
    const username = `${expiry}:${randomBytes(12).toString("hex")}`;
    const credential = createHmac("sha1", env.TURN_SHARED_SECRET)
      .update(username)
      .digest("base64");
    iceServers.push({ urls: turn, username, credential });
    expiresAt = new Date(expiry * 1000).toISOString();
  }
  if (env.VOICE_RELAY_ONLY === "true" && !turn.length)
    throw new Error("Relay-only mode requires TURN");
  return {
    iceServers,
    turnConfigured: turn.length > 0,
    expiresAt,
    relayOnly: env.VOICE_RELAY_ONLY === "true",
  };
}
const router = express.Router();
router.get("/config", protectRoute, (_req, res) => {
  res.set("Cache-Control", "no-store");
  try {
    res.json(voiceConfiguration());
  } catch {
    res
      .status(503)
      .json({
        message:
          "Voice network configuration is incomplete. Ask the operator to configure TURN.",
      });
  }
});
export default router;
