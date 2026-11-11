import express from "express";
import { areFriends, disappearingExpiry, pairDisappearAfter } from "../lib/friends.js";
import mongoose from "mongoose";
import Attachment from "../models/attachment.model.js";
import Community from "../models/community.model.js";
import User from "../models/user.model.js";
import { protectRoute } from "../middleware/auth.middleware.js";
import { userRateLimit } from "../middleware/userRateLimit.js";

// Files are encrypted by the sender, so this only ever stores unreadable bytes.
// The limits protect the free 512 MB database, which messages also use.
export const MAX_FILE_BYTES = 8 * 1024 * 1024;
const OVERHEAD = 16; // The AES-GCM authentication tag.
export const PER_USER_BYTES = 64 * 1024 * 1024;
export const TOTAL_BYTES = 256 * 1024 * 1024;

const router = express.Router();
const valid = (value) => typeof value === "string" && /^[a-f0-9]{24}$/i.test(value);
const usedBy = async (match) => (await Attachment.aggregate([{ $match: match }, { $group: { _id: null, bytes: { $sum: "$size" } } }]))[0]?.bytes || 0;

router.use(protectRoute);
router.post(
  "/",
  userRateLimit({ limit: 30, windowMs: 60000 }),
  express.raw({ type: "application/octet-stream", limit: MAX_FILE_BYTES + OVERHEAD }),
  async (req, res) => {
    const { to, community, channel } = req.query;
    if (!Buffer.isBuffer(req.body) || req.body.length <= OVERHEAD || Boolean(to) === Boolean(community))
      return res.status(400).json({ message: "Invalid file upload" });
    if (!valid(to || community)) return res.status(400).json({ message: "Invalid file upload" });
    try {
      if (to) {
        if (!(await areFriends(req.user._id, to))) return res.status(403).json({ message: "You can only send files to friends" });
      } else if (!(await Community.exists({ _id: community, members: req.user._id }))) {
        return res.status(404).json({ message: "Community unavailable" });
      }
      if ((await usedBy({ ownerId: req.user._id })) + req.body.length > PER_USER_BYTES)
        return res.status(413).json({ message: "You have reached your file limit. Older files clear after 7 days." });
      if ((await usedBy({})) + req.body.length > TOTAL_BYTES)
        return res.status(507).json({ message: "File storage is full right now. Older files clear after 7 days." });
      // A file in a disappearing conversation disappears with its message.
      let seconds = to ? await pairDisappearAfter(req.user._id, to) : 0;
      if (community && valid(channel)) seconds = (await Community.findById(community).select("channels").lean())?.channels.find((item) => String(item._id) === channel)?.disappearAfter || 0;
      const attachment = await Attachment.create({ ownerId: req.user._id, recipientId: to, communityId: community, size: req.body.length, data: req.body, ...disappearingExpiry(seconds) });
      res.status(201).json({ id: attachment._id });
    } catch {
      res.status(503).json({ message: "Could not store the file" });
    }
  },
);

router.get("/:id", async (req, res) => {
  const gone = () => res.status(404).json({ message: "This file is no longer available" });
  if (!valid(req.params.id)) return gone();
  try {
    const file = await Attachment.findById(req.params.id).select("+data");
    if (!file) return gone();
    const me = String(req.user._id);
    const allowed = String(file.ownerId) === me || String(file.recipientId) === me ||
      (file.communityId && await Community.exists({ _id: file.communityId, members: req.user._id, banned: { $ne: req.user._id } }));
    if (!allowed) return gone();
    res.set("Content-Type", "application/octet-stream");
    res.set("Content-Disposition", "attachment");
    res.send(file.data);
  } catch {
    res.status(503).json({ message: "Could not load the file" });
  }
});

router.delete("/:id", async (req, res) => {
  if (!valid(req.params.id) || !mongoose.isValidObjectId(req.params.id)) return res.status(404).json({ message: "File unavailable" });
  try {
    await Attachment.deleteOne({ _id: req.params.id, ownerId: req.user._id });
    res.json({ ok: true });
  } catch {
    res.status(503).json({ message: "Could not delete the file" });
  }
});

// Oversized uploads are rejected before the handler runs.
router.use((error, _req, res, next) => {
  if (error?.type === "entity.too.large") return res.status(413).json({ message: "Files can be up to 8 MB" });
  next(error);
});

export default router;
