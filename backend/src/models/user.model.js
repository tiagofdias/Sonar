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
    // Unique public handle; see lib/usernames.js.
    username: { type: String, unique: true, sparse: true },
    // A sea animal the person chose. Without one, apps pick one from the account id.
    animal: { type: String, default: null },
    preferences: {
      type: new mongoose.Schema(
        {
          mutedCommunities: [{ type: mongoose.Schema.Types.ObjectId }],
          pinnedPeople: [{ type: mongoose.Schema.Types.ObjectId }],
          folders: [{ _id: false, name: String, people: [{ type: mongoose.Schema.Types.ObjectId }] }],
          typingHidden: { type: Boolean, default: false },
        },
        { _id: false },
      ),
      default: () => ({}),
      select: false,
    },
    blocked: { type: [{ type: mongoose.Schema.Types.ObjectId, ref: "User" }], default: [], select: false },
    presence: { type: String, enum: ["online", "away", "busy", "offline"], default: "online" },
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
