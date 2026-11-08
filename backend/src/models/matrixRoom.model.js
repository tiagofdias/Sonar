import mongoose from "mongoose";

// Internal Matrix rooms, separate from the existing Sonar history.
const schema = new mongoose.Schema({
  roomId: { type: String, required: true },
  owner: { type: String, required: true },
  members: { type: [String], required: true },
  kind: { type: String, enum: ["direct", "community"], required: true },
  revision: { type: Number, default: 0 },
  activityVersion: { type: Number, default: 0 },
}, { timestamps: true, versionKey: false });
schema.index({ roomId: 1 }, { unique: true });
schema.index({ members: 1 });
export default mongoose.model("MatrixRoom", schema);
