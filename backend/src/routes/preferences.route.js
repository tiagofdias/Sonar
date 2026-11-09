import express from "express";
import mongoose from "mongoose";
import { protectRoute } from "../middleware/auth.middleware.js";
import User from "../models/user.model.js";
import Friendship from "../models/friendship.model.js";
import PushSubscription from "../models/pushSubscription.model.js";
import { pairKey } from "../lib/friends.js";
import { allowedEndpoint, pushKeys } from "../lib/push.js";
import { io } from "../lib/socket.js";

const router = express.Router();
const validId = (value) => typeof value === "string" && /^[a-f0-9]{24}$/i.test(value);
const idList = (value, limit) => Array.isArray(value) && value.length <= limit && value.every(validId);
const PUBLIC = "fullName username animal";
const changed = (id) => io.to(String(id)).emit("preferencesChanged", {});
const friendsChanged = (...ids) => ids.forEach((id) => io.to(String(id)).emit("friendsChanged", {}));

router.use(protectRoute);

const current = async (userId) => {
  const user = await User.findById(userId).select("+preferences +blocked").lean();
  const blocked = await User.find({ _id: { $in: user?.blocked || [] } }).select(PUBLIC).lean();
  const saved = user?.preferences || {};
  return { mutedCommunities: saved.mutedCommunities || [], pinnedPeople: saved.pinnedPeople || [], folders: saved.folders || [], typingHidden: Boolean(saved.typingHidden), blocked };
};

router.get("/", async (req, res) => {
  try { res.json(await current(req.user._id)); }
  catch { res.status(500).json({ message: "Could not load your preferences" }); }
});

router.put("/", async (req, res) => {
  const { mutedCommunities, pinnedPeople, folders, typingHidden } = req.body;
  const set = {};
  if (mutedCommunities !== undefined) {
    if (!idList(mutedCommunities, 200)) return res.status(400).json({ message: "Invalid muted servers" });
    set["preferences.mutedCommunities"] = mutedCommunities;
  }
  if (pinnedPeople !== undefined) {
    if (!idList(pinnedPeople, 100)) return res.status(400).json({ message: "Invalid pinned conversations" });
    set["preferences.pinnedPeople"] = pinnedPeople;
  }
  if (folders !== undefined) {
    const valid = Array.isArray(folders) && folders.length <= 20 && folders.every((folder) => typeof folder?.name === "string" && folder.name.trim() && folder.name.length <= 30 && idList(folder.people, 100));
    if (!valid) return res.status(400).json({ message: "Invalid folders" });
    set["preferences.folders"] = folders.map(({ name, people }) => ({ name: name.trim(), people }));
  }
  if (typingHidden !== undefined) {
    if (typeof typingHidden !== "boolean") return res.status(400).json({ message: "Invalid typing setting" });
    set["preferences.typingHidden"] = typingHidden;
  }
  try {
    await User.updateOne({ _id: req.user._id }, { $set: set });
    changed(req.user._id);
    res.json(await current(req.user._id));
  } catch { res.status(500).json({ message: "Could not save your preferences" }); }
});

router.post("/block/:id", async (req, res) => {
  const target = req.params.id;
  if (!mongoose.isValidObjectId(target) || target === String(req.user._id)) return res.status(400).json({ message: "Choose another person" });
  try {
    if (!(await User.exists({ _id: target }))) return res.status(404).json({ message: "That person is unavailable" });
    const pair = pairKey(req.user._id, target);
    // The row stays, so people who were already talking are never reconnected at startup.
    await Friendship.updateOne({ pair }, { $set: { status: "blocked", requestedBy: req.user._id, users: [req.user._id, target] }, $unset: { note: "" } }, { upsert: true });
    await User.updateOne({ _id: req.user._id }, { $addToSet: { blocked: target } });
    friendsChanged(req.user._id, target);
    changed(req.user._id);
    res.json(await current(req.user._id));
  } catch { res.status(500).json({ message: "Could not block that person" }); }
});

router.delete("/block/:id", async (req, res) => {
  const target = req.params.id;
  if (!mongoose.isValidObjectId(target)) return res.status(400).json({ message: "Choose another person" });
  try {
    await User.updateOne({ _id: req.user._id }, { $pull: { blocked: target } });
    const pair = pairKey(req.user._id, target);
    const theyBlockedMe = await User.exists({ _id: target, blocked: req.user._id });
    if (theyBlockedMe) await Friendship.updateOne({ pair, status: "blocked" }, { $set: { requestedBy: target } });
    else await Friendship.deleteOne({ pair, status: "blocked" });
    friendsChanged(req.user._id, target);
    changed(req.user._id);
    res.json(await current(req.user._id));
  } catch { res.status(500).json({ message: "Could not unblock that person" }); }
});

router.get("/push-key", async (_req, res) => {
  try { res.json({ publicKey: (await pushKeys()).publicKey }); }
  catch { res.status(503).json({ message: "Alerts are unavailable right now" }); }
});

router.post("/push", async (req, res) => {
  const { endpoint, keys } = req.body;
  if (typeof endpoint !== "string" || endpoint.length > 800 || !allowedEndpoint(endpoint) || typeof keys?.p256dh !== "string" || keys.p256dh.length > 200 || typeof keys?.auth !== "string" || keys.auth.length > 100)
    return res.status(400).json({ message: "This browser's alert service is not supported" });
  try {
    await PushSubscription.updateOne({ endpoint }, { $set: { userId: req.user._id, keys: { p256dh: keys.p256dh, auth: keys.auth } } }, { upsert: true });
    res.json({ ok: true });
  } catch { res.status(500).json({ message: "Could not turn on alerts" }); }
});

router.delete("/push", async (req, res) => {
  try {
    await PushSubscription.deleteOne({ endpoint: String(req.body.endpoint || ""), userId: req.user._id });
    res.json({ ok: true });
  } catch { res.status(500).json({ message: "Could not turn off alerts" }); }
});

export default router;
