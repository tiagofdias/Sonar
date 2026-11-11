import express from "express";
import mongoose from "mongoose";
import { protectRoute } from "../middleware/auth.middleware.js";
import User from "../models/user.model.js";
import Friendship from "../models/friendship.model.js";
import { DISAPPEAR_OPTIONS, pairKey } from "../lib/friends.js";
import { io } from "../lib/socket.js";

const router = express.Router();
const MAX_OUTGOING = 100;
const PUBLIC = "fullName username animal encryptionKey";
const validNote = (note) => note && note.version === 1 && typeof note.iv === "string" && /^[A-Za-z0-9+/]{16}$/.test(note.iv) && typeof note.ciphertext === "string" && note.ciphertext.length >= 24 && note.ciphertext.length <= 2000 && /^[A-Za-z0-9+/]+={0,2}$/.test(note.ciphertext);
const changed = (...ids) => ids.forEach((id) => io.to(String(id)).emit("friendsChanged", {}));

router.get("/", protectRoute, async (req, res) => {
  try {
    const rows = await Friendship.find({ users: req.user._id, status: "pending" }).sort({ createdAt: -1 }).limit(500).lean();
    const other = (row) => String(row.users.find((id) => String(id) !== String(req.user._id)));
    const people = await User.find({ _id: { $in: rows.map(other) } }).select(PUBLIC).lean();
    const byId = new Map(people.map((person) => [String(person._id), person]));
    const pick = (mine) => rows.filter((row) => (String(row.requestedBy) === String(req.user._id)) === mine).flatMap((row) => {
      const person = byId.get(other(row));
      if (!person) return [];
      const { encryptionKey, ...shown } = person;
      // The key is only handed over to open a note this person was sent.
      return [{ ...shown, ...(!mine && row.note ? { note: row.note, encryptionKey } : {}) }];
    });
    res.json({ incoming: pick(false), outgoing: pick(true) });
  } catch {
    res.status(500).json({ message: "Could not load friend requests" });
  }
});

// Lets a request carry a note that only its recipient can read.
router.get("/key/:username", protectRoute, async (req, res) => {
  const username = String(req.params.username).trim().replace(/^@/, "").toLowerCase();
  try {
    const target = await User.findOne({ username }).select(PUBLIC).lean();
    if (!target) return res.status(404).json({ message: "No one has that username" });
    res.json(target);
  } catch {
    res.status(500).json({ message: "Could not find that person" });
  }
});

router.post("/request", protectRoute, async (req, res) => {
  const username = typeof req.body.username === "string" ? req.body.username.trim().replace(/^@/, "").toLowerCase() : "";
  if (!username || username.length > 64) return res.status(400).json({ message: "Enter a username" });
  if (req.body.note !== undefined && !validNote(req.body.note)) return res.status(400).json({ message: "That note could not be sent" });
  try {
    const target = await User.findOne({ username }).select(PUBLIC);
    if (!target) return res.status(404).json({ message: "No one has that username" });
    if (String(target._id) === String(req.user._id)) return res.status(400).json({ message: "That's your own username" });
    const pair = pairKey(req.user._id, target._id);
    const existing = await Friendship.findOne({ pair });
    if (existing?.status === "blocked") {
      if (String(existing.requestedBy) === String(req.user._id)) return res.status(409).json({ message: "You blocked this person. Unblock them first" });
      // Someone who was blocked is not told.
      return res.status(201).json({ status: "pending" });
    }
    if (existing?.status === "accepted") return res.status(409).json({ message: "You're already friends" });
    if (existing?.status === "removed") {
      // A former friend asks again: a fresh request the other person has to accept.
      if ((await Friendship.countDocuments({ requestedBy: req.user._id, status: "pending" })) >= MAX_OUTGOING)
        return res.status(429).json({ message: "Too many requests are waiting for a reply" });
      existing.set({ status: "pending", requestedBy: req.user._id, users: [req.user._id, target._id], note: req.body.note || undefined });
      await existing.save();
      io.to(String(target._id)).emit("friendRequest", { fullName: req.user.fullName });
      changed(req.user._id, target._id);
      return res.status(201).json({ status: "pending" });
    }
    if (existing && String(existing.requestedBy) === String(req.user._id)) return res.status(409).json({ message: "Request already sent" });
    if (existing) {
      // They had already asked, so asking back is the same as accepting.
      existing.status = "accepted";
      await existing.save();
      changed(req.user._id, target._id);
      return res.json({ status: "accepted" });
    }
    if ((await Friendship.countDocuments({ requestedBy: req.user._id, status: "pending" })) >= MAX_OUTGOING)
      return res.status(429).json({ message: "Too many requests are waiting for a reply" });
    await Friendship.create({ pair, users: [req.user._id, target._id], requestedBy: req.user._id, status: "pending", ...(req.body.note ? { note: req.body.note } : {}) });
    io.to(String(target._id)).emit("friendRequest", { fullName: req.user.fullName });
    changed(req.user._id, target._id);
    res.status(201).json({ status: "pending" });
  } catch (error) {
    if (error?.code === 11000) return res.status(409).json({ message: "Request already sent" });
    res.status(500).json({ message: "Could not send the friend request" });
  }
});

router.post("/:id/accept", protectRoute, async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id)) return res.status(400).json({ message: "Invalid request" });
  try {
    const row = await Friendship.findOneAndUpdate(
      { pair: pairKey(req.user._id, req.params.id), status: "pending", requestedBy: req.params.id },
      { $set: { status: "accepted" }, $unset: { note: "" } },
    );
    if (!row) return res.status(404).json({ message: "That request is no longer available" });
    changed(req.user._id, req.params.id);
    res.json({ status: "accepted" });
  } catch {
    res.status(500).json({ message: "Could not accept the request" });
  }
});

router.put("/:id/disappear", protectRoute, async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id) || !DISAPPEAR_OPTIONS.includes(req.body.seconds)) return res.status(400).json({ message: "Choose a valid time" });
  try {
    const row = await Friendship.findOneAndUpdate({ pair: pairKey(req.user._id, req.params.id), status: "accepted" }, { $set: { disappearAfter: req.body.seconds } });
    if (!row) return res.status(404).json({ message: "You can only change this with friends" });
    changed(req.user._id, req.params.id);
    res.json({ seconds: req.body.seconds });
  } catch {
    res.status(500).json({ message: "Could not change disappearing messages" });
  }
});

// Ends a friendship. Either person can send a new request later.
router.delete("/:id/friendship", protectRoute, async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id)) return res.status(400).json({ message: "Invalid request" });
  try {
    const row = await Friendship.findOneAndUpdate(
      { pair: pairKey(req.user._id, req.params.id), status: "accepted" },
      { $set: { status: "removed", requestedBy: req.user._id }, $unset: { note: "" } },
    );
    if (!row) return res.status(404).json({ message: "You're not friends with this person" });
    changed(req.user._id, req.params.id);
    res.json({ status: "removed" });
  } catch {
    res.status(500).json({ message: "Could not remove this friend" });
  }
});

// Declines a request sent to you, or cancels one you sent.
router.delete("/:id", protectRoute, async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id)) return res.status(400).json({ message: "Invalid request" });
  try {
    const row = await Friendship.findOneAndDelete({ pair: pairKey(req.user._id, req.params.id), status: "pending" });
    if (!row) return res.status(404).json({ message: "That request is no longer available" });
    changed(req.user._id, req.params.id);
    res.json({ status: "removed" });
  } catch {
    res.status(500).json({ message: "Could not remove the request" });
  }
});

export default router;
