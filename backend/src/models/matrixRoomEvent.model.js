import mongoose from "mongoose";
import { WEEK } from "./message.model.js";

const schema = new mongoose.Schema({
  roomId: { type: String, required: true },
  sender: { type: String, required: true },
  senderDevice: { type: String, required: true },
  // Members who could receive the group key at the time of the send.
  visibleTo: { type: [String], required: true },
  content: { type: mongoose.Schema.Types.Mixed, required: true },
  // Only encrypted bytes. Keeping them in the event makes TTL, pinning and
  // deletion atomic with the message; history never returns the blob itself.
  attachmentCiphertext: { type: Buffer, select: false },
  attachmentId: { type: String },
  pinned: { type: Boolean, default: false },
  expiresAt: { type: Date, default: () => new Date(Date.now() + WEEK) },
}, { timestamps: true, versionKey: false });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
schema.index({ roomId: 1, createdAt: -1, _id: -1 });
schema.index({ attachmentId: 1 }, { unique: true, sparse: true });
export default mongoose.model("MatrixRoomEvent", schema);
