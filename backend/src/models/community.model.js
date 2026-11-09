import mongoose from "mongoose";
const community = new mongoose.Schema(
  {
    name: { type: String, required: true, maxlength: 60 },
    // A group chat between friends uses the same encrypted channels as a server.
    kind: { type: String, enum: ["server", "group"], default: "server" },
    moderatorLimits: [{ _id: false, userId: mongoose.Schema.Types.ObjectId, denied: [String] }],
    activity: { type: [{ _id: false, name: String, action: String, at: Date }], select: false, default: [] },
    ownerId: {
      type: mongoose.Schema.Types.ObjectId,
      ref: "User",
      required: true,
    },
    members: [{ type: mongoose.Schema.Types.ObjectId, ref: "User" }],
    moderators: [{ type: mongoose.Schema.Types.ObjectId, ref: "User" }],
    banned: { type: [{ type: mongoose.Schema.Types.ObjectId, ref: "User" }], select: false, default: [] },
    revision: { type: Number, default: 0 },
    activityVersion: { type: Number, default: 0 },
    channels: [
      {
        name: { type: String, required: true, maxlength: 40 },
        type: { type: String, enum: ["text", "voice"], default: "text" },
        locked: { type: Boolean, default: false },
        disappearAfter: { type: Number, default: 0 },
      },
    ],
    inviteHash: { type: String, select: false },
    inviteExpiresAt: { type: Date, select: false },
  },
  { timestamps: true },
);
community.index({ members: 1 });
export default mongoose.model("Community", community);
