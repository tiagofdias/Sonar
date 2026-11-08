import mongoose from "mongoose";
import { WEEK } from "./message.model.js";
const envelope = new mongoose.Schema(
  {
    recipientId: { type: mongoose.Schema.Types.ObjectId, required: true },
    ciphertext: { type: String, required: true, maxlength: 24000 },
    iv: { type: String, required: true },
    version: { type: Number, required: true, enum: [1] },
  },
  { _id: false },
);
const message = new mongoose.Schema(
  {
    communityId: { type: mongoose.Schema.Types.ObjectId, required: true },
    channelId: { type: mongoose.Schema.Types.ObjectId, required: true },
    senderId: {
      type: mongoose.Schema.Types.ObjectId,
      required: true,
      ref: "User",
    },
    envelopes: { type: [envelope], required: true },
    pinned: { type: Boolean, default: false },
    expiresAt: { type: Date, default: () => new Date(Date.now() + WEEK) },
  },
  { timestamps: true },
);
message.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
message.index({ communityId: 1, channelId: 1, createdAt: -1 });
export default mongoose.model("ChannelMessage", message);
