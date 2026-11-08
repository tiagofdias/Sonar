import express from "express";
import mongoose from "mongoose";
import { randomBytes, createHash } from "node:crypto";
import Community from "../models/community.model.js";
import ChannelMessage from "../models/channelMessage.model.js";
import User from "../models/user.model.js";
import { WEEK } from "../models/message.model.js";
import { protectRoute } from "../middleware/auth.middleware.js";
import { io } from "../lib/socket.js";
import { historyQuery, historyPage, pinUpdate } from "../lib/history.js";
import { reconcileVoiceRooms } from "../lib/voiceRooms.js";

const router = express.Router();
const hash = (value) => createHash("sha256").update(value).digest("hex");
const id = (value) =>
  typeof value === "string" && /^[a-f0-9]{24}$/i.test(value);
const live = () => ({
  $or: [{ pinned: true }, { expiresAt: { $gt: new Date() } }],
});
const safe = (handler) => async (req, res, next) => {
  try {
    await handler(req, res, next);
  } catch (error) {
    if (error.code === 11000)
      return res.status(409).json({ message: "Please refresh and try again" });
    res.status(500).json({ message: "Community request failed" });
  }
};
const notify = (community) => {
  reconcileVoiceRooms(community);
  for (const member of community.members)
    io.to(String(member)).emit("communityUpdated", {
      communityId: String(community._id),
    });
};
export function forRecipient(message, recipientId) {
  const data = message.toObject ? message.toObject() : message;
  const envelope = data.envelopes.find(
    (e) => String(e.recipientId) === String(recipientId),
  );
  if (!envelope) return null;
  const { envelopes, ...metadata } = data;
  return { ...metadata, ...envelope, receiverId: String(recipientId) };
}
export function validEnvelopes(envelopes, members) {
  if (
    !Array.isArray(envelopes) ||
    envelopes.length !== members.length ||
    envelopes.length > 32
  )
    return false;
  const recipients = new Set(envelopes.map((e) => e?.recipientId));
  return (
    recipients.size === members.length &&
    members.every((m) => recipients.has(String(m))) &&
    envelopes.every(
      (e) =>
        e.version === 1 &&
        typeof e.iv === "string" &&
        /^[A-Za-z0-9+/]{16}$/.test(e.iv) &&
        typeof e.ciphertext === "string" &&
        e.ciphertext.length >= 24 &&
        e.ciphertext.length <= 24000 &&
        /^[A-Za-z0-9+/]+={0,2}$/.test(e.ciphertext) &&
        !e.text &&
        !e.image,
    )
  );
}
router.use(protectRoute);
router.get(
  "/",
  safe(async (req, res) => {
    res.json(
      await Community.find({ members: req.user._id }).sort({ createdAt: 1 }),
    );
  }),
);
router.post(
  "/",
  safe(async (req, res) => {
    if (
      typeof req.body.name !== "string" ||
      !req.body.name.trim() ||
      req.body.name.trim().length > 60
    )
      return res
        .status(400)
        .json({ message: "Use a community name between 1 and 60 characters" });
    if (!req.user.encryptionKey)
      return res
        .status(409)
        .json({ message: "Set up browser encryption first" });
    const community = await Community.create({
      name: req.body.name.trim(),
      ownerId: req.user._id,
      members: [req.user._id],
      channels: [{ name: "general" }],
    });
    res.status(201).json(community);
  }),
);
router.post(
  "/join",
  safe(async (req, res) => {
    if (
      typeof req.body.token !== "string" ||
      !/^[a-f0-9]{48}$/.test(req.body.token)
    )
      return res.status(400).json({ message: "Invalid invite code" });
    if (!req.user.encryptionKey)
      return res
        .status(409)
        .json({ message: "Set up browser encryption first" });
    const criteria = {
      inviteHash: hash(req.body.token),
      inviteExpiresAt: { $gt: new Date() },
      banned: { $ne: req.user._id },
    };
    let community = await Community.findOneAndUpdate(
      {
        ...criteria,
        members: { $ne: req.user._id },
        $expr: { $lt: [{ $size: "$members" }, 32] },
      },
      { $addToSet: { members: req.user._id }, $inc: { revision: 1 } },
      { new: true },
    );
    if (!community)
      community = await Community.findOne({
        ...criteria,
        members: req.user._id,
      });
    if (!community)
      return res
        .status(404)
        .json({ message: "Invite expired, revoked, or community full" });
    notify(community);
    res.json(community);
  }),
);
router.use(
  "/:communityId",
  safe(async (req, res, next) => {
    if (!id(req.params.communityId))
      return res.status(404).json({ message: "Community unavailable" });
    req.community = await Community.findOne({
      _id: req.params.communityId,
      members: req.user._id,
    });
    if (!req.community)
      return res.status(404).json({ message: "Community unavailable" });
    next();
  }),
);
router.get(
  "/:communityId",
  safe(async (req, res) => {
    const authorIds = await ChannelMessage.distinct("senderId", {
      communityId: req.community._id,
      "envelopes.recipientId": req.user._id,
      ...live(),
    });
    const members = await User.find({
      _id: { $in: req.community.members },
    }).select("fullName encryptionKey");
    const historyAuthors = await User.find({ _id: { $in: authorIds } }).select(
      "fullName encryptionKey",
    );
    res.json({
      ...req.community.toObject(),
      memberDetails: members,
      historyAuthors,
    });
  }),
);
router.get(
  "/:communityId/unread-candidates",
  safe(async (req, res) => {
    const channelIds = req.community.channels.filter(channel => channel.type !== "voice").map(channel => channel._id);
    const rows = await ChannelMessage.aggregate([
      { $match: { communityId: req.community._id, channelId: { $in: channelIds }, senderId: { $ne: req.user._id }, "envelopes.recipientId": req.user._id, ...live() } },
      { $sort: { channelId: 1, createdAt: -1, _id: -1 } },
      { $group: { _id: "$channelId", messageId: { $first: "$_id" }, createdAt: { $first: "$createdAt" } } },
      { $project: { _id: 0, channelId: "$_id", messageId: 1, createdAt: 1 } },
    ]);
    res.json(rows);
  }),
);
router.post(
  "/:communityId/leave",
  safe(async (req, res) => {
    if (String(req.community.ownerId) === String(req.user._id))
      return res
        .status(409)
        .json({ message: "Transfer ownership before leaving this community" });
    const community = await Community.findOneAndUpdate(
      {
        _id: req.community._id,
        ownerId: { $ne: req.user._id },
        members: req.user._id,
      },
      {
        $pull: { members: req.user._id, moderators: req.user._id },
        $inc: { revision: 1 },
      },
      { new: true },
    );
    if (!community)
      return res
        .status(409)
        .json({ message: "Membership changed. Refresh and try again." });
    io.to(String(req.user._id)).emit("communityRemoved", {
      communityId: String(community._id),
    });
    notify(community);
    res.json({ success: true });
  }),
);
router.delete(
  "/:communityId/members/:memberId",
  safe(async (req, res) => {
    const owner = String(req.community.ownerId) === String(req.user._id);
    const moderator = req.community.moderators.some(member => String(member) === String(req.user._id));
    if (!owner && !moderator)
      return res.status(403).json({ message: "Only the owner or a moderator can remove members" });
    if (
      !id(req.params.memberId) ||
      req.params.memberId === String(req.user._id)
    )
      return res.status(400).json({
        message:
          "You cannot remove yourself. Use leave community instead.",
      });
    const community = await Community.findOneAndUpdate(
      {
        _id: req.community._id,
        ownerId: { $ne: req.params.memberId },
        $and: [
          { members: req.user._id },
          { members: req.params.memberId },
          { $or: [
            { ownerId: req.user._id },
            { $and: [{ moderators: req.user._id }, { moderators: { $ne: req.params.memberId } }] },
          ] },
        ],
      },
      {
        $pull: { members: req.params.memberId, moderators: req.params.memberId },
        $inc: { revision: 1 },
        $unset: { inviteHash: "", inviteExpiresAt: "" },
      },
      { new: true },
    );
    if (!community)
      return res.status(409).json({
        message: "Membership or ownership changed. Refresh and try again.",
      });
    io.to(req.params.memberId).emit("communityRemoved", {
      communityId: String(community._id),
    });
    notify(community);
    res.json({ success: true });
  }),
);
router.post(
  "/:communityId/members/:memberId/ban",
  safe(async (req, res) => {
    const owner = String(req.community.ownerId) === String(req.user._id);
    const moderator = req.community.moderators.some(member => String(member) === String(req.user._id));
    if (!owner && !moderator) return res.status(403).json({ message: "Only the owner or a moderator can ban members" });
    const target = req.params.memberId;
    if (!id(target) || target === String(req.user._id)) return res.status(400).json({ message: "Choose another member" });
    const community = await Community.findOneAndUpdate({
      _id: req.community._id,
      ownerId: { $ne: target },
      $expr: { $lt: [{ $size: { $ifNull: ["$banned", []] } }, 500] },
      $and: [
        { members: req.user._id }, { members: target },
        { $or: [ { ownerId: req.user._id },
          { $and: [{ moderators: req.user._id }, { moderators: { $ne: target } }] } ] },
      ],
    }, {
      $pull: { members: target, moderators: target },
      $addToSet: { banned: target },
      $inc: { revision: 1 },
      $unset: { inviteHash: "", inviteExpiresAt: "" },
    }, { new: true });
    if (!community) return res.status(409).json({ message: "Membership or permissions changed, or the ban list is full." });
    io.to(target).emit("communityRemoved", { communityId: String(community._id), reason: "banned" });
    notify(community);
    res.json({ success: true });
  }),
);
router.get(
  "/:communityId/bans",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id)) return res.status(403).json({ message: "Only the owner can view bans" });
    const current = await Community.findOne({ _id: req.community._id, ownerId: req.user._id }).select("+banned");
    if (!current) return res.status(404).json({ message: "Community unavailable" });
    const users = await User.find({ _id: { $in: current.banned } }).select("fullName").lean();
    const names = new Map(users.map(user => [String(user._id), user.fullName]));
    res.json(current.banned.map(userId => ({ _id: String(userId), fullName: names.get(String(userId)) || "Deleted account" })));
  }),
);
router.delete(
  "/:communityId/bans/:memberId",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id)) return res.status(403).json({ message: "Only the owner can remove bans" });
    const target = req.params.memberId;
    if (!id(target)) return res.status(404).json({ message: "Ban unavailable" });
    const community = await Community.findOneAndUpdate({ _id: req.community._id, ownerId: req.user._id, banned: target },
      { $pull: { banned: target }, $unset: { inviteHash: "", inviteExpiresAt: "" }, $inc: { revision: 1 } }, { new: true });
    if (!community) return res.status(404).json({ message: "Ban unavailable" });
    notify(community);
    res.json({ success: true });
  }),
);
router.put(
  "/:communityId/members/:memberId/moderator",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res.status(403).json({ message: "Only the owner can manage moderators" });
    const memberId = req.params.memberId;
    if (!id(memberId) || memberId === String(req.user._id) || typeof req.body.moderator !== "boolean")
      return res.status(400).json({ message: "Choose another member and a valid role" });
    const grant = req.body.moderator;
    const community = await Community.findOneAndUpdate(
      { _id: req.community._id, ownerId: req.user._id, members: memberId,
        moderators: grant ? { $ne: memberId } : memberId },
      { ...(grant ? { $addToSet: { moderators: memberId } } : { $pull: { moderators: memberId } }),
        $inc: { revision: 1 } },
      { new: true },
    );
    if (!community) return res.status(409).json({ message: "Membership or role changed. Refresh and try again." });
    notify(community);
    res.json({ success: true });
  }),
);
router.post(
  "/:communityId/transfer",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res
        .status(403)
        .json({ message: "Only the owner can transfer ownership" });
    const { memberId } = req.body;
    if (!id(memberId) || memberId === String(req.user._id))
      return res.status(400).json({ message: "Choose another current member" });
    const community = await Community.findOneAndUpdate(
      { _id: req.community._id, ownerId: req.user._id, members: memberId },
      {
        ownerId: memberId,
        $pull: { moderators: memberId },
        $inc: { revision: 1 },
        $unset: { inviteHash: "", inviteExpiresAt: "" },
      },
      { new: true },
    );
    if (!community)
      return res.status(409).json({
        message: "Membership or ownership changed. Refresh and try again.",
      });
    notify(community);
    res.json({ success: true });
  }),
);
router.delete(
  "/:communityId",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res.status(403).json({ message: "Only the owner can delete the community" });
    let removed;
    await mongoose.connection.transaction(async session => {
      removed = await Community.findOneAndDelete({ _id: req.community._id, ownerId: req.user._id }, { session });
      if (removed) await ChannelMessage.deleteMany({ communityId: removed._id }, { session });
    });
    if (!removed) return res.status(404).json({ message: "Community unavailable" });
    reconcileVoiceRooms({ _id: removed._id, revision: removed.revision + 1, members: [], channels: [] });
    const event = { communityId: String(removed._id), reason: "deleted" };
    for (const member of removed.members) io.to(String(member)).emit("communityRemoved", event);
    res.json(event);
  }),
);
router.patch(
  "/:communityId",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res.status(403).json({ message: "Only the owner can rename the community" });
    const name = typeof req.body.name === "string" ? req.body.name.trim() : "";
    if (!name || name.length > 60)
      return res.status(400).json({ message: "Use a community name between 1 and 60 characters" });
    const community = await Community.findOneAndUpdate(
      { _id: req.community._id, ownerId: req.user._id }, { $set: { name } }, { new: true },
    );
    if (!community) return res.status(409).json({ message: "Ownership changed. Refresh and try again" });
    notify(community);
    res.json(community);
  }),
);
router.patch(
  "/:communityId/channels/:channelId",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res.status(403).json({ message: "Only the owner can rename channels" });
    if (!id(req.params.channelId) || !req.community.channels.id(req.params.channelId))
      return res.status(404).json({ message: "Channel not found" });
    const name = typeof req.body.name === "string" ? req.body.name.trim().toLowerCase().replace(/\s+/g, "-") : "";
    if (!/^[a-z0-9][a-z0-9-]{0,39}$/.test(name))
      return res.status(400).json({ message: "Use 1-40 letters, numbers, or hyphens" });
    const community = await Community.findOneAndUpdate(
      { _id: req.community._id, ownerId: req.user._id,
        "channels._id": req.params.channelId,
        channels: { $not: { $elemMatch: { name, _id: { $ne: req.params.channelId } } } } },
      { $set: { "channels.$.name": name } }, { new: true },
    );
    if (!community) return res.status(409).json({ message: "Channel name already exists or access changed" });
    notify(community);
    res.json(community);
  }),
);
router.patch(
  "/:communityId/channels/:channelId/voice-lock",
  safe(async (req, res) => {
    if (!id(req.params.channelId) || typeof req.body.locked !== "boolean")
      return res.status(400).json({ message: "Invalid voice room request" });
    const isOwner = String(req.community.ownerId) === String(req.user._id);
    const isModerator = req.community.moderators.some(id => String(id) === String(req.user._id));
    if (!isOwner && !isModerator) return res.status(403).json({ message: "Only the owner or a moderator can lock voice rooms" });
    const community = await Community.findOneAndUpdate({
      _id: req.community._id, members: req.user._id,
      $or: [{ ownerId: req.user._id }, { moderators: req.user._id }],
      channels: { $elemMatch: { _id: req.params.channelId, type: "voice" } },
    }, { $set: { "channels.$[target].locked": req.body.locked }, $inc: { revision: 1 } },
      { new: true, arrayFilters: [{ "target._id": req.params.channelId, "target.type": "voice" }] });
    if (!community) return res.status(404).json({ message: "Voice room unavailable or permissions changed" });
    notify(community);
    res.json({ success: true, locked: req.body.locked });
  }),
);
router.delete(
  "/:communityId/channels/:channelId",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res.status(403).json({ message: "Only the owner can delete channels" });
    if (!id(req.params.channelId)) return res.status(404).json({ message: "Channel unavailable" });
    let updated, outcome;
    await mongoose.connection.transaction(async session => {
      updated = null;
      outcome = "missing";
      const current = await Community.findOne({ _id: req.community._id, ownerId: req.user._id }).session(session);
      const channel = current?.channels.id(req.params.channelId);
      if (!channel) return;
      if (channel.type !== "voice" && current.channels.filter(item => item.type !== "voice").length <= 1) {
        outcome = "lastText";
        return;
      }
      updated = await Community.findOneAndUpdate(
        { _id: current._id, ownerId: req.user._id, revision: current.revision, "channels._id": channel._id },
        { $pull: { channels: { _id: channel._id } }, $inc: { revision: 1 } },
        { new: true, session },
      );
      if (!updated) { outcome = "changed"; return; }
      await ChannelMessage.deleteMany({ communityId: current._id, channelId: channel._id }, { session });
      outcome = "deleted";
    });
    if (outcome === "lastText") return res.status(409).json({ message: "Keep at least one text channel" });
    if (outcome !== "deleted") return res.status(404).json({ message: "Channel unavailable" });
    notify(updated);
    res.json({ communityId: String(updated._id), channelId: req.params.channelId });
  }),
);
router.post(
  "/:communityId/channels",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res
        .status(403)
        .json({ message: "Only the owner can create channels" });
    const name =
      typeof req.body.name === "string"
        ? req.body.name.trim().toLowerCase().replace(/\s+/g, "-")
        : "";
    const type = req.body.type || "text";
    if (!["text", "voice"].includes(type))
      return res
        .status(400)
        .json({ message: "Choose a text channel or voice room" });
    if (!/^[a-z0-9][a-z0-9-]{0,39}$/.test(name))
      return res
        .status(400)
        .json({ message: "Use 1-40 letters, numbers, or hyphens" });
    const community = await Community.findOneAndUpdate(
      {
        _id: req.community._id,
        ownerId: req.user._id,
        "channels.name": { $ne: name },
        $expr: { $lt: [{ $size: "$channels" }, 30] },
      },
      { $push: { channels: { name, type } } },
      { new: true },
    );
    if (!community)
      return res
        .status(409)
        .json({ message: "Channel already exists or channel limit reached" });
    notify(community);
    res.status(201).json(community);
  }),
);
router.post(
  "/:communityId/invite",
  safe(async (req, res) => {
    if (String(req.community.ownerId) !== String(req.user._id))
      return res
        .status(403)
        .json({ message: "Only the owner can create invites" });
    const token = randomBytes(24).toString("hex");
    const expiresAt = new Date(Date.now() + 24 * 60 * 60 * 1000);
    const result = await Community.updateOne(
      { _id: req.community._id, ownerId: req.user._id },
      { inviteHash: hash(token), inviteExpiresAt: expiresAt },
    );
    if (!result.matchedCount)
      return res
        .status(409)
        .json({ message: "Ownership changed. Refresh and try again." });
    res.json({ token, expiresAt });
  }),
);
router.use("/:communityId/channels/:channelId", (req, res, next) => {
  if (
    !id(req.params.channelId) ||
    !req.community.channels.id(req.params.channelId) ||
    req.community.channels.id(req.params.channelId)?.type === "voice"
  )
    return res.status(404).json({ message: "Channel unavailable" });
  next();
});
router.get(
  "/:communityId/channels/:channelId/messages",
  safe(async (req, res) => {
    let options;
    try {
      options = historyQuery(req.query);
    } catch {
      return res
        .status(400)
        .json({ message: "Invalid history cursor or options" });
    }
    const messages = await ChannelMessage.find({
      communityId: req.community._id,
      channelId: req.params.channelId,
      "envelopes.recipientId": req.user._id,
      ...live(),
      ...(options.filters.length ? { $and: options.filters } : {}),
    })
      .sort({ createdAt: -1, _id: -1 })
      .limit(options.limit + 1);
    const page = historyPage(messages, options.limit, (m) =>
      forRecipient(m, req.user._id),
    );
    res.json(options.paged ? page : page.messages);
  }),
);
router.post(
  "/:communityId/channels/:channelId/messages",
  safe(async (req, res) => {
    const { envelopes, revision } = req.body;
    if (revision !== req.community.revision)
      return res.status(409).json({
        message: "Membership changed. Refresh the community and resend.",
      });
    if (req.body.text || !validEnvelopes(envelopes, req.community.members))
      return res
        .status(400)
        .json({ message: "Encrypt one envelope for every current member" });
    let message;
    try {
      await mongoose.connection.transaction(async (session) => {
        // Every send writes the community document, serializing against membership changes.
        const current = await Community.findOneAndUpdate(
          {
            _id: req.community._id,
            revision,
            members: req.user._id,
            "channels._id": req.params.channelId,
          },
          { $inc: { activityVersion: 1 } },
          { new: true, session },
        );
        if (!current || !validEnvelopes(envelopes, current.members)) {
          const error = new Error("Membership changed");
          error.membershipChanged = true;
          throw error;
        }
        [message] = await ChannelMessage.create(
          [
            {
              communityId: current._id,
              channelId: req.params.channelId,
              senderId: req.user._id,
              envelopes,
            },
          ],
          { session },
        );
      });
    } catch (error) {
      if (error.membershipChanged)
        return res.status(409).json({
          message: "Membership changed. Refresh the community and resend.",
        });
      throw error;
    }
    const current = await Community.findById(req.community._id);
    for (const member of current?.members || [])
      if (forRecipient(message, member))
        io.to(String(member)).emit(
          "channelMessageNew",
          forRecipient(message, member),
        );
    res.status(201).json(forRecipient(message, req.user._id));
  }),
);
router.patch(
  "/:communityId/channels/:channelId/messages/:messageId/pin",
  safe(async (req, res) => {
    if (!id(req.params.messageId) || typeof req.body.pinned !== "boolean")
      return res.status(400).json({ message: "Invalid pin request" });
    const message = await ChannelMessage.findOneAndUpdate(
      {
        _id: req.params.messageId,
        communityId: req.community._id,
        channelId: req.params.channelId,
        "envelopes.recipientId": req.user._id,
        ...live(),
      },
      pinUpdate(req.body.pinned, WEEK),
      { new: true },
    );
    if (!message)
      return res.status(404).json({ message: "Message unavailable" });
    const current = await Community.findById(req.community._id);
    for (const member of current?.members || []) {
      const envelope = forRecipient(message, member);
      if (envelope) io.to(String(member)).emit("channelMessage", envelope);
    }
    res.json(forRecipient(message, req.user._id));
  }),
);
router.delete(
  "/:communityId/channels/:channelId/messages/:messageId",
  safe(async (req, res) => {
    if (!id(req.params.messageId)) return res.status(400).json({ message: "Invalid message" });
    let message;
    await mongoose.connection.transaction(async session => {
      const community = await Community.findOneAndUpdate(
        { _id: req.community._id, members: req.user._id, "channels._id": req.params.channelId },
        { $inc: { activityVersion: 1 } }, { new: true, session },
      );
      if (!community) return;
      message = await ChannelMessage.findOneAndDelete({
        _id: req.params.messageId, communityId: community._id,
        channelId: req.params.channelId, senderId: req.user._id,
      }, { session });
    });
    if (!message) return res.status(404).json({ message: "Message unavailable" });
    const event = { id: String(message._id), communityId: String(message.communityId), channelId: String(message.channelId) };
    const current = await Community.findById(req.community._id);
    for (const member of current?.members || []) io.to(String(member)).emit("channelMessageDeleted", event);
    res.json(event);
  }),
);
export default router;
