import mongoose from "mongoose";

// Internal v2 crypto state; not mounted on public routes until device approval
// and session binding have been implemented.
const schema = new mongoose.Schema({
  userId: { type: String, required: true },
  deviceId: { type: String, required: true },
  deviceKeys: { type: mongoose.Schema.Types.Mixed, required: true },
  revision: { type: Number, default: 0 },
  serverAuthorization: { type: String, enum: ["unmanaged", "pending", "authorized"], default: "unmanaged" },
  authorizedBy: { type: String, default: null },
  authorizedAt: { type: Date, default: null },
  revokedAt: { type: Date, default: null },
}, { timestamps: true });
schema.index({ userId: 1, deviceId: 1 }, { unique: true });
export default mongoose.model("MatrixDevice", schema);
