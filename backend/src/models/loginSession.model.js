import mongoose from "mongoose";
const schema = new mongoose.Schema({
  _id: String,
  userId: { type: mongoose.Schema.Types.ObjectId, required: true, index: true },
  sessionVersion: { type: Number, required: true },
  method: { type: String, enum: ["password", "passkey"], required: true },
  expiresAt: { type: Date, required: true },
  matrixDevice: { userId: { type: String }, deviceId: { type: String } },
  matrixChallengeRevision: { type: Number, default: 0 },
}, { timestamps: { createdAt: true, updatedAt: false }, versionKey: false });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("LoginSession", schema);
