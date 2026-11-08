import mongoose from "mongoose";

// A shared write serializes attachment quota checks across rooms/processes.
const schema = new mongoose.Schema({
  _id: { type: String, required: true },
  revision: { type: Number, required: true, default: 0 },
}, { versionKey: false });
export default mongoose.model("MatrixStorageBudget", schema);
