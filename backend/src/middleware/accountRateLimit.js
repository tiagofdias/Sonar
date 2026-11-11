import { createHash } from "node:crypto";
// Limits attempts against one account (by email), wherever they come from, so a
// password cannot be guessed from many addresses. The per-address limit still applies.
export function createAccountRateLimit({ limit, windowMs = 15 * 60 * 1000, now = Date.now, maxKeys = 20000 }) {
  const attempts = new Map();
  return (req, res, next) => {
    const email = typeof req.body?.email === "string" ? req.body.email.trim().toLowerCase().slice(0, 254) : "";
    if (!email) return next();
    const key = createHash("sha256").update(email).digest("hex");
    const time = now();
    let entry = attempts.get(key);
    if (!entry || entry.expiresAt <= time) {
      if (attempts.size >= maxKeys) for (const [id, value] of attempts) if (value.expiresAt <= time) attempts.delete(id);
      if (attempts.size >= maxKeys) attempts.clear();
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
export const accountLoginLimit = createAccountRateLimit({ limit: 20 });
export const accountRecoverLimit = createAccountRateLimit({ limit: 10 });
