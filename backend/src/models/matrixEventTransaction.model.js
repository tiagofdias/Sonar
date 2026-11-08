import mongoose from "mongoose";

// Opaque server-issued transaction receipts expire independently of messages.
// They contain only a ciphertext hash and an event identifier, never content.
const schema = new mongoose.Schema({
  tokenHash: { type: String, required: true },
  roomId: { type: String, required: true },
  sender: { type: String, required: true },
  senderDevice: { type: String, required: true },
  revision: { type: Number, required: true },
  contentHash: { type: String, default: null },
  eventId: { type: mongoose.Schema.Types.ObjectId, default: null },
  expiresAt: { type: Date, required: true },
}, { versionKey: false });
schema.index({ tokenHash: 1 }, { unique: true });
schema.index({ sender: 1, senderDevice: 1, expiresAt: 1 });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("MatrixEventTransaction", schema);
