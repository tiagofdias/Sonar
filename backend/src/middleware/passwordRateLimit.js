import { selectableRateLimit } from "./sharedRateLimit.js";
// Process-local protection for the current-password verification endpoint.
// Multi-instance deployments should use a shared rate-limit store.
const attempts = new Map();
const windowMs = 15 * 60 * 1000;
function localPasswordRateLimit(req, res, next) {
  const now = Date.now(),
    key = String(req.user._id);
  let entry = attempts.get(key);
  if (!entry || entry.expiresAt <= now) {
    if (attempts.size >= 10000)
      for (const [id, item] of attempts) {
        if (item.expiresAt <= now) attempts.delete(id);
      }
    if (attempts.size >= 10000)
      return res.status(503).json({ message: "Please try again later" });
    entry = { count: 0, expiresAt: now + windowMs };
    attempts.set(key, entry);
  }
  if (++entry.count > 10) {
    res.set("Retry-After", String(Math.ceil((entry.expiresAt - now) / 1000)));
    return res
      .status(429)
      .json({
        message: "Too many password attempts. Try again in 15 minutes.",
      });
  }
  next();
}

export const passwordRateLimit = selectableRateLimit({ scope: "password-confirmation", limit: 10, keyFor: req => req.user._id, local: localPasswordRateLimit });
