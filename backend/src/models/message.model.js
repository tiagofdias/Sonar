import mongoose from "mongoose";
export const WEEK = 7 * 24 * 60 * 60 * 1000;
const schema = new mongoose.Schema(
  {
    senderId: {
      type: mongoose.Schema.Types.ObjectId,
      ref: "User",
      required: true,
    },
    receiverId: {
      type: mongoose.Schema.Types.ObjectId,
      ref: "User",
      required: true,
    },
    ciphertext: { type: String, required: true, maxlength: 24000 },
    iv: { type: String, required: true },
    version: { type: Number, default: 1, enum: [1] },
    pinned: { type: Boolean, default: false },
    editedAt: { type: Date, default: null },
    expiresAt: { type: Date, default: () => new Date(Date.now() + WEEK) },
  },
  { timestamps: true },
);
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
schema.index({ senderId: 1, receiverId: 1, createdAt: 1 });
schema.index({ receiverId: 1, senderId: 1, createdAt: -1, _id: -1 });
export default mongoose.model("Message", schema);
