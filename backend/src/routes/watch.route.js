import express from "express";
import jwt from "jsonwebtoken";
import { protectRoute } from "../middleware/auth.middleware.js";
import { userRateLimit } from "../middleware/userRateLimit.js";
import { createAuthRateLimit } from "../middleware/authRateLimit.js";
import { createWatchLink, redeemWatchLink } from "../lib/watch.js";

// Lets the desktop app open Watch Together in the person's own browser without
// signing in there: a one-time link becomes a cookie that only allows watching.
const router = express.Router();
const WATCH_MS = 6 * 60 * 60 * 1000;

router.post("/link", protectRoute, userRateLimit({ limit: 10, windowMs: 60000 }), (req, res) => {
  const link = createWatchLink(req.user._id, req.user.sessionVersion || 0);
  if (!link) return res.status(409).json({ message: "Join a call or voice room to watch together." });
  res.json({ path: `/watch#t=${link.token}` });
});

router.post("/redeem", createAuthRateLimit({ limit: 30 }), (req, res) => {
  const link = redeemWatchLink(req.body?.token);
  if (!link) return res.status(410).json({ message: "This watch link has expired. Open it again from Sonar." });
  const token = jwt.sign({ userId: link.userId, scope: link.scope, sessionVersion: link.sessionVersion, purpose: "watch" }, process.env.JWT_SECRET, { expiresIn: WATCH_MS / 1000, algorithm: "HS256" });
  res.cookie("sonar_watch", token, { maxAge: WATCH_MS, httpOnly: true, sameSite: "strict", secure: process.env.NODE_ENV === "production" });
  res.json({ ok: true, userId: link.userId });
});

export default router;
