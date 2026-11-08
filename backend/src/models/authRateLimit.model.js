import mongoose from "mongoose";
const schema = new mongoose.Schema({
  _id: String,
  count: { type: Number, required: true },
  expiresAt: { type: Date, required: true },
}, { versionKey: false });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("AuthRateLimit", schema);
