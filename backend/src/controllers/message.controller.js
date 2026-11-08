import mongoose from "mongoose";
import User from "../models/user.model.js";
import Message, { WEEK } from "../models/message.model.js";
import { io } from "../lib/socket.js";
import { historyQuery, historyPage, pinUpdate } from "../lib/history.js";
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
    res.json(
      await User.find({ _id: { $ne: req.user._id } }).select(
        "fullName username profilePic encryptionKey",
      ),
    );
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
    });
    io.to(String(recipient._id))
      .to(String(req.user._id))
      .emit("newMessage", message);
    res.status(201).json(message);
  } catch {
    res.status(500).json({ message: "Message could not be sent" });
  }
};
export const pinMessage = async (req, res) => {
  if (
    !mongoose.isValidObjectId(req.params.id) ||
    typeof req.body.pinned !== "boolean"
  )
    return res.status(400).json({ message: "Invalid pin request" });
  try {
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
