import mongoose from "mongoose";

// One row per pair of people. Declining or cancelling a request deletes the row.
const friendshipSchema = new mongoose.Schema(
  {
    pair: { type: String, required: true, unique: true },
    users: { type: [mongoose.Schema.Types.ObjectId], ref: "User", required: true, index: true },
    requestedBy: { type: mongoose.Schema.Types.ObjectId, ref: "User" },
    status: { type: String, enum: ["pending", "accepted"], required: true },
  },
  { timestamps: true },
);

export default mongoose.model("Friendship", friendshipSchema);
