import mongoose from "mongoose";

// Where a browser receives alerts. Push messages never carry sender or message text.
const schema = new mongoose.Schema(
  {
    userId: { type: mongoose.Schema.Types.ObjectId, ref: "User", required: true, index: true },
    endpoint: { type: String, required: true, unique: true, maxlength: 800 },
    keys: { p256dh: { type: String, required: true, maxlength: 200 }, auth: { type: String, required: true, maxlength: 100 } },
  },
  { timestamps: true },
);
export default mongoose.model("PushSubscription", schema);

const config = new mongoose.Schema({ _id: String, publicKey: String, privateKey: String });
export const PushConfig = mongoose.model("PushConfig", config);
