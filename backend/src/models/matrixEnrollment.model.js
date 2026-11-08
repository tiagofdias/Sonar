import mongoose from "mongoose";

const schema = new mongoose.Schema({
  tokenHash: { type: String, required: true },
  accountId: { type: mongoose.Schema.Types.ObjectId, required: true },
  sessionId: { type: String, required: true },
  userId: { type: String, required: true },
  deviceId: { type: String, required: true },
  deviceKeys: { type: mongoose.Schema.Types.Mixed, required: true },
  keysHash: { type: String, required: true },
  failures: { type: Number, default: 0 },
  consumedAt: { type: Date, default: null },
  expiresAt: { type: Date, required: true },
}, { versionKey: false });
schema.index({ tokenHash: 1 }, { unique: true });
schema.index({ sessionId: 1, expiresAt: 1 });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("MatrixEnrollment", schema);
