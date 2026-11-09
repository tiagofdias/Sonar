import mongoose from "mongoose";
import { WEEK } from "./message.model.js";

// An encrypted file. The key lives only inside the end-to-end encrypted message.
const schema = new mongoose.Schema(
  {
    ownerId: { type: mongoose.Schema.Types.ObjectId, ref: "User", required: true, index: true },
    recipientId: { type: mongoose.Schema.Types.ObjectId, ref: "User" },
    communityId: { type: mongoose.Schema.Types.ObjectId, ref: "Community" },
    size: { type: Number, required: true },
    data: { type: Buffer, required: true, select: false },
    expiresAt: { type: Date, default: () => new Date(Date.now() + WEEK) },
  },
  { timestamps: true },
);
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("Attachment", schema);
