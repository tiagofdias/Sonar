import mongoose from "mongoose";
import User from "../models/user.model.js";
import Message, { WEEK } from "../models/message.model.js";
import { io } from "../lib/socket.js";
import { historyQuery, historyPage, pinUpdate } from "../lib/history.js";
import { areFriends, disappearingExpiry, pairDisappearAfter } from "../lib/friends.js";
import Friendship from "../models/friendship.model.js";
import { pushTo } from "../lib/push.js";
const live = () => ({
  $or: [{ pinned: true }, { expiresAt: { $gt: new Date() } }],
});
// Metadata only. Read positions remain on the client and are not read receipts.
export const getUnreadCandidates = async (req, res) => {
  try {
    const after = req.query.after;
    if (after !== undefined && (typeof after !== "string" || !/^[a-f0-9]{24}$/.test(after))) return res.status(400).json({ message: "Invalid unread cursor" });
    const sender = { $ne: req.user._id, ...(after ? { $gt: new mongoose.Types.ObjectId(after) } : {}) };
    const rows = await Message.aggregate([
      { $match: { receiverId: req.user._id, senderId: sender, ...live() } },
      { $sort: { senderId: 1, createdAt: -1, _id: -1 } },
      { $group: { _id: "$senderId", messageId: { $first: "$_id" }, createdAt: { $first: "$createdAt" } } },
      { $sort: { _id: 1 } },
      { $limit: 101 },
      { $project: { _id: 0, senderId: "$_id", messageId: 1, createdAt: 1 } },
    ]);
    const hasMore = rows.length > 100;
    const items = rows.slice(0, 100);
    res.json({ items, next: hasMore ? String(items.at(-1).senderId) : null });
  } catch {
    res.status(500).json({ message: "Could not load unread indicators" });
  }
};
export const getUsersForSidebar = async (req, res) => {
  try {
    const rows = await Friendship.find({ users: req.user._id, status: "accepted" }).select("users disappearAfter").lean();
    const after = new Map(rows.map((row) => [String(row.users.find((id) => String(id) !== String(req.user._id))), row.disappearAfter || 0]));
    const people = await User.find({ _id: { $in: [...after.keys()] } }).select("fullName username profilePic encryptionKey animal").lean();
    res.json(people.map((person) => ({ ...person, disappearAfter: after.get(String(person._id)) || 0 })));
  } catch {
    res.status(500).json({ message: "Could not load people" });
  }
};
export const getMessages = async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id))
    return res.status(400).json({ message: "Invalid recipient" });
  let options;
  try {
    options = historyQuery(req.query);
  } catch {
    return res
      .status(400)
      .json({ message: "Invalid history cursor or options" });
  }
  try {
    if (!(await areFriends(req.user._id, req.params.id)))
      return res.status(403).json({ message: "You can only open conversations with friends" });
    const rows = await Message.find({
      $and: [
        live(),
        {
          $or: [
            { senderId: req.user._id, receiverId: req.params.id },
            { senderId: req.params.id, receiverId: req.user._id },
          ],
        },
        ...options.filters,
      ],
    })
      .sort({ createdAt: -1, _id: -1 })
      .limit(options.limit + 1);
    const page = historyPage(rows, options.limit);
    res.json(options.paged ? page : page.messages);
  } catch {
    res.status(500).json({ message: "Could not load messages" });
  }
};
export const sendMessage = async (req, res) => {
  const { ciphertext, iv, version } = req.body;
  if (
    req.body.text ||
    req.body.image ||
    version !== 1 ||
    typeof ciphertext !== "string" ||
    ciphertext.length < 24 ||
    ciphertext.length > 24000 ||
    !/^[A-Za-z0-9+/]+={0,2}$/.test(ciphertext) ||
    typeof iv !== "string" ||
    !/^[A-Za-z0-9+/]{16}$/.test(iv) ||
    !mongoose.isValidObjectId(req.params.id)
  )
    return res
      .status(400)
      .json({ message: "Only encrypted message envelopes are accepted" });
  try {
    if (!(await areFriends(req.user._id, req.params.id)))
      return res.status(403).json({ message: "You can only message friends" });
    const recipient = await User.findById(req.params.id);
    if (!req.user.encryptionKey || !recipient?.encryptionKey)
      return res
        .status(409)
        .json({ message: "Both people must set up encryption first" });
    const message = await Message.create({
      senderId: req.user._id,
      receiverId: recipient._id,
      ciphertext,
      iv,
      version,
      ...disappearingExpiry(await pairDisappearAfter(req.user._id, recipient._id)),
    });
    pushTo(recipient._id, "message", { sender: req.user._id });
    io.to(String(recipient._id))
      .to(String(req.user._id))
      .emit("newMessage", message);
    res.status(201).json(message);
  } catch {
    res.status(500).json({ message: "Message could not be sent" });
  }
};
export const editMessage = async (req, res) => {
  const { ciphertext, iv, version } = req.body;
  if (version !== 1 || typeof ciphertext !== "string" || ciphertext.length < 24 || ciphertext.length > 24000 || !/^[A-Za-z0-9+/]+={0,2}$/.test(ciphertext) ||
      typeof iv !== "string" || !/^[A-Za-z0-9+/]{16}$/.test(iv) || !mongoose.isValidObjectId(req.params.id))
    return res.status(400).json({ message: "Only encrypted message envelopes are accepted" });
  try {
    const message = await Message.findOneAndUpdate(
      { _id: req.params.id, senderId: req.user._id, ...live() },
      { $set: { ciphertext, iv, version, editedAt: new Date() } },
      { new: true },
    );
    if (!message) return res.status(404).json({ message: "Message unavailable" });
    io.to(String(message.senderId)).to(String(message.receiverId)).emit("messageUpdated", message);
    res.json(message);
  } catch {
    res.status(500).json({ message: "Could not edit the message" });
  }
};
export const pinMessage = async (req, res) => {
  if (
    !mongoose.isValidObjectId(req.params.id) ||
    typeof req.body.pinned !== "boolean"
  )
    return res.status(400).json({ message: "Invalid pin request" });
  try {
    if (req.body.pinned) {
      const existing = await Message.findById(req.params.id).select("senderId receiverId").lean();
      if (existing && await pairDisappearAfter(existing.senderId, existing.receiverId))
        return res.status(409).json({ message: "Messages cannot be pinned while disappearing messages are on" });
    }
    const message = await Message.findOneAndUpdate(
      {
        _id: req.params.id,
        $and: [
          live(),
          { $or: [{ senderId: req.user._id }, { receiverId: req.user._id }] },
        ],
      },
      pinUpdate(req.body.pinned, WEEK),
      { new: true },
    );
    if (!message)
      return res.status(404).json({ message: "Message unavailable" });
    io.to(String(message.senderId))
      .to(String(message.receiverId))
      .emit("messageUpdated", message);
    res.json(message);
  } catch {
    res.status(500).json({ message: "Could not update pin" });
  }
};

export const deleteMessage = async (req, res) => {
  if (!mongoose.isValidObjectId(req.params.id))
    return res.status(400).json({ message: "Invalid message" });
  try {
    const message = await Message.findOneAndDelete({ _id: req.params.id, senderId: req.user._id });
    if (!message) return res.status(404).json({ message: "Message unavailable" });
    const event = { id: String(message._id), senderId: String(message.senderId), receiverId: String(message.receiverId) };
    io.to(event.senderId).to(event.receiverId).emit("messageDeleted", event);
    res.json(event);
  } catch {
    res.status(500).json({ message: "Could not delete message" });
  }
};
