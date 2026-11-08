import mongoose from "mongoose";

// Keep receipts after acknowledgement to prevent a retry from redelivering keys.
const schema = new mongoose.Schema({
  sender: { type: String, required: true },
  deviceId: { type: String, required: true },
  transactionId: { type: String, required: true },
  digest: { type: String, required: true },
  expiresAt: { type: Date, required: true },
}, { versionKey: false });
schema.index({ sender: 1, deviceId: 1, transactionId: 1 }, { unique: true });
schema.index({ expiresAt: 1 }, { expireAfterSeconds: 0 });
export default mongoose.model("MatrixToDeviceTransaction", schema);
