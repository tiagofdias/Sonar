import mongoose from "mongoose";
const schema = new mongoose.Schema(
  {
    userId: {
      type: mongoose.Schema.Types.ObjectId,
      ref: "User",
      required: true,
      index: true,
    },
    credentialId: { type: String, required: true, unique: true },
    publicKey: { type: Buffer, required: true, select: false },
    counter: { type: Number, required: true, default: 0 },
    transports: [String],
    name: { type: String, required: true, maxlength: 60 },
    rpID: { type: String, required: true },
    lastUsedAt: Date,
  },
  { timestamps: true },
);
export default mongoose.model("Passkey", schema);
