import mongoose from "mongoose";

// Session identifiers are retained to reject old group keys after membership
// changes. They contain no message body; room deletion must remove them.
const schema = new mongoose.Schema({
  roomId: { type: String, required: true },
  sender: { type: String, required: true },
  senderDevice: { type: String, required: true },
  sessionId: { type: String, required: true },
  revision: { type: Number, required: true },
}, { versionKey: false });
schema.index({ roomId: 1, sender: 1, senderDevice: 1, sessionId: 1 }, { unique: true });
export default mongoose.model("MatrixRoomSession", schema);
