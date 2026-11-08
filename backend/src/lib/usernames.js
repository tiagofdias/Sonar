import { randomBytes } from "node:crypto";
import User from "../models/user.model.js";

// Public, unique handles: 3-24 lowercase letters, digits, "_" or ".".
export function normalizeUsername(value) {
  if (typeof value !== "string") return null;
  const name = value.trim().replace(/^@/, "").toLowerCase();
  return /^[a-z0-9_.]{3,24}$/.test(name) ? name : null;
}

export const USERNAME_RULES = "Usernames use 3 to 24 letters, numbers, dots or underscores";

// Derives a free username from a display name or email.
export async function availableUsername(seed) {
  let base = String(seed || "").split("@")[0].toLowerCase().normalize("NFD")
    .replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9_.]/g, "").slice(0, 20);
  if (base.length < 3) base = `user${base}`;
  for (let i = 1; i <= 50; i++) {
    const candidate = i === 1 ? base : `${base}${i}`;
    if (!(await User.exists({ username: candidate }))) return candidate;
  }
  return `${base.slice(0, 14)}${randomBytes(4).toString("hex")}`;
}

// Gives every account created before usernames existed a handle at startup.
export async function ensureUsernames() {
  const missing = await User.find({ $or: [{ username: null }, { username: { $exists: false } }] }).select("fullName email").lean();
  for (const user of missing) {
    for (let attempt = 0; attempt < 5; attempt++) {
      try {
        await User.updateOne({ _id: user._id }, { $set: { username: await availableUsername(user.fullName || user.email) } });
        break;
      } catch (error) {
        if (error.code !== 11000 || attempt === 4) throw error; // Another instance took it: try again.
      }
    }
  }
}
