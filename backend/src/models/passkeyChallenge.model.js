import mongoose from "mongoose";
const schema = new mongoose.Schema({
  tokenHash: { type: String, required: true, unique: true },
  challenge: { type: String, required: true },
  purpose: {
    type: String,
    enum: ["registration", "authentication"],
    required: true,
  },
  userId: mongoose.Schema.Types.ObjectId,
  sessionVersion: Number,
  name: String,
  origin: { type: String, required: true },
  rpID: { type: String, required: true },
  expiresAt: { type: Date, required: true },
});
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("PasskeyChallenge", schema);
