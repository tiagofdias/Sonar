import mongoose from "mongoose";

// One row per pair of people. Declining or cancelling a request deletes the row.
const friendshipSchema = new mongoose.Schema(
  {
    pair: { type: String, required: true, unique: true },
    users: { type: [mongoose.Schema.Types.ObjectId], ref: "User", required: true, index: true },
    requestedBy: { type: mongoose.Schema.Types.ObjectId, ref: "User" },
    status: { type: String, enum: ["pending", "accepted", "blocked"], required: true },
    // Encrypted for the recipient, like a message. Only kept while the request is waiting.
    note: { type: new mongoose.Schema({ ciphertext: String, iv: String, version: Number }, { _id: false }), default: undefined },
    // Seconds until new messages between these two people disappear. Zero means the normal seven days.
    disappearAfter: { type: Number, default: 0 },
  },
  { timestamps: true },
);

export default mongoose.model("Friendship", friendshipSchema);
