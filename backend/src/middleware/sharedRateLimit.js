import { createHmac } from "node:crypto";
import RateLimit from "../models/authRateLimit.model.js";

let indexPromise;
function ensureExpiryIndex() {
  if (!indexPromise)
    indexPromise = RateLimit.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 0 })
      .catch(error => { indexPromise = null; throw error; });
  return indexPromise;
}

// Fixed windows are shared across instances using the same MongoDB and JWT secret.
// Database failures fail closed; there is no silent per-process fallback.
export function createSharedRateLimit({ scope, limit, windowMs = 15 * 60 * 1000, keyFor = req => req.ip || req.socket?.remoteAddress || "unknown", now = Date.now, secret = () => process.env.JWT_SECRET }) {
  return async (req, res, next) => {
    const time = now();
    const expiresAt = new Date((Math.floor(time / windowMs) + 1) * windowMs);
    try {
      const key = secret();
      if (!key) throw new Error("Missing limiter secret");
      const id = createHmac("sha256", key).update(JSON.stringify([scope, String(keyFor(req)), expiresAt.getTime()])).digest("hex");
      await ensureExpiryIndex();
      let entry;
      const query = () => RateLimit.findOneAndUpdate(
        { _id: id }, { $inc: { count: 1 }, $setOnInsert: { expiresAt } },
        { new: true, upsert: true },
      );
      try { entry = await query(); }
      catch (error) { if (error.code !== 11000) throw error; entry = await query(); }
      if (entry.count > limit) {
        res.set("Retry-After", String(Math.max(1, Math.ceil((expiresAt.getTime() - time) / 1000))));
        return res.status(429).json({ message: "Too many attempts. Please try again later." });
      }
      next();
    } catch {
      res.status(503).json({ message: "Account protection is temporarily unavailable. Please try again later." });
    }
  };
}
export function selectableRateLimit({ scope, limit, windowMs, keyFor, local }) {
  const shared = createSharedRateLimit({ scope, limit, windowMs, keyFor });
  return (req, res, next) => process.env.MONGO_SHARED_AUTH_LIMITS === "true"
    ? shared(req, res, next)
    : local(req, res, next);
}
