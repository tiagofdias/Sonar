import mongoose from "mongoose";

const schema = new mongoose.Schema({
  userId: { type: String, required: true },
  deviceId: { type: String, required: true },
  keyId: { type: String, required: true },
  signedKey: { type: mongoose.Schema.Types.Mixed, required: true },
  // Tombstones prevent a retried upload from resurrecting a claimed key.
  claimedAt: { type: Date, default: null },
}, { timestamps: true });
schema.index({ userId: 1, deviceId: 1, keyId: 1 }, { unique: true });
schema.index({ userId: 1, deviceId: 1, claimedAt: 1, createdAt: 1 });
export default mongoose.model("MatrixOneTimeKey", schema);
