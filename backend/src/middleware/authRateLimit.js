import { selectableRateLimit } from "./sharedRateLimit.js";
// In-memory limits are per process. Configure trusted proxies explicitly at deployment.
export function createAuthRateLimit({ limit, windowMs = 15 * 60 * 1000, now = Date.now, maxKeys = 10000 }) {
  const attempts = new Map();
  return (req, res, next) => {
    const time = now();
    const key = req.ip || req.socket?.remoteAddress || "unknown";
    let entry = attempts.get(key);
    if (!entry || entry.expiresAt <= time) {
      if (attempts.size >= maxKeys) {
        for (const [id, value] of attempts) if (value.expiresAt <= time) attempts.delete(id);
      }
      if (!entry && attempts.size >= maxKeys)
        return res.status(503).json({ message: "Too many requests. Please try again later." });
      entry = { count: 0, expiresAt: time + windowMs };
      attempts.set(key, entry);
    }
    if (++entry.count > limit) {
      res.set("Retry-After", String(Math.max(1, Math.ceil((entry.expiresAt - time) / 1000))));
      return res.status(429).json({ message: "Too many attempts. Please try again later." });
    }
    next();
  };
}
export const loginRateLimit = selectableRateLimit({ scope: "login", limit: 30, local: createAuthRateLimit({ limit: 30 }) });
export const signupRateLimit = selectableRateLimit({ scope: "signup", limit: 10, local: createAuthRateLimit({ limit: 10 }) });
