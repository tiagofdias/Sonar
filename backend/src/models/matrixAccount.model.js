import mongoose from "mongoose";

// A permanent bootstrap marker prevents removing the last authorized device
// from silently turning a subsequent registration into another first device.
const schema = new mongoose.Schema({
  _id: String,
  bootstrapped: { type: Boolean, default: false },
  revision: { type: Number, default: 0 },
}, { versionKey: false });
export default mongoose.model("MatrixAccount", schema);
