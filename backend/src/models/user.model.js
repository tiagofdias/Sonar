import mongoose from "mongoose";

const userSchema = new mongoose.Schema(
  {
    encryptionKey: { type: String, default: null },
    // Client-encrypted private key (see lib/keyBackup.js). Never plaintext.
    keyBackup: {
      type: new mongoose.Schema(
        {
          version: Number,
          publicKey: String,
          salt: String,
          iv: String,
          ciphertext: String,
          iterations: Number,
        },
        { _id: false },
      ),
      default: null,
      select: false,
    },
    // SHA-256 of the recovery proof (see lib/keyBackup.js); resets a forgotten password.
    recoveryVerifier: { type: String, default: null, select: false },
    sessionVersion: { type: Number, default: 0 },
    passkeyRevision: { type: Number, default: 0 },
    matrixRevision: { type: Number, default: 0 },
    email: {
      type: String,
      required: true,
      unique: true,
    },
    fullName: {
      type: String,
      required: true,
    },
    password: {
      type: String,
      required: true,
      minlength: 6,
    },
    profilePic: {
      type: String,
      default: "",
    },
  },
  { timestamps: true },
);

const User = mongoose.model("User", userSchema);

export default User;
