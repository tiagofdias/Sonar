import mongoose from "mongoose";

// Encrypted to-device events are ephemeral even if MongoDB's TTL monitor is late.
const schema = new mongoose.Schema({
  sender: { type: String, required: true },
  recipient: { type: String, required: true },
  deviceId: { type: String, required: true },
  purpose: { type: String, enum: ["general", "verification", "contact-verification"], default: "general" },
  roomId: { type: String, default: null },
  content: { type: mongoose.Schema.Types.Mixed, required: true },
  expiresAt: { type: Date, required: true },
}, { timestamps: { createdAt: true, updatedAt: false }, versionKey: false });
schema.index({ recipient: 1, deviceId: 1, _id: 1 });
schema.index({ recipient: 1, deviceId: 1, purpose: 1, roomId: 1, _id: 1 });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("MatrixToDevice", schema);
