// Per-account limits for signed-in requests. Use after protectRoute. In-memory and
// per process, like the sign-in limits; it bounds abuse from any single account.
export function userRateLimit({ limit, windowMs = 60000, now = Date.now, maxKeys = 20000 }) {
  const attempts = new Map();
  return (req, res, next) => {
    const key = String(req.user?._id || req.ip || "unknown");
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
      return res.status(429).json({ message: "You're doing that too fast. Please wait a moment." });
    }
    next();
  };
}
