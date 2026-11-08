import express from "express";
import bcrypt from "bcryptjs";
import User from "../models/user.model.js";
import { validPublicKey, parseKeyBackup, publicBackup, validVerifier, proofMatches, RECOVERY_BACKUP_VERSION } from "../lib/keyBackup.js";
import { generateToken } from "../lib/utils.js";
import {
  checkAuth,
  login,
  logout,
  signup,
  updateProfile,
  changePassword,
} from "../controllers/auth.controller.js";
import { protectRoute } from "../middleware/auth.middleware.js";
import { passwordRateLimit } from "../middleware/passwordRateLimit.js";
import { loginRateLimit, signupRateLimit } from "../middleware/authRateLimit.js";
import passkeyRoutes from "./passkey.route.js";
import LoginSession from "../models/loginSession.model.js";
import { io } from "../lib/socket.js";

const router = express.Router();
router.use("/passkeys", passkeyRoutes);

router.post("/signup", signupRateLimit, signup);
router.post("/login", loginRateLimit, login);
router.post("/logout", protectRoute, logout);

router.get("/sessions", protectRoute, async (req, res) => {
  try {
    const rows = await LoginSession.find({ userId: req.user._id, sessionVersion: req.user.sessionVersion || 0, expiresAt: { $gt: new Date() } })
      .sort({ createdAt: -1 }).limit(101).lean();
    res.json({ sessions: rows.slice(0, 100).map(row => ({ id: row._id, method: row.method, createdAt: row.createdAt, expiresAt: row.expiresAt, current: row._id === req.sessionId })), hasMore: rows.length > 100, legacyCurrent: !req.sessionId });
  } catch { res.status(503).json({ message: "Could not load sessions" }); }
});
router.delete("/sessions/:id", protectRoute, async (req, res) => {
  if (!/^[a-f0-9]{64}$/.test(req.params.id)) return res.status(404).json({ message: "Session unavailable" });
  try {
    const session = await LoginSession.findOneAndDelete({ _id: req.params.id, userId: req.user._id, sessionVersion: req.user.sessionVersion || 0 });
    if (!session) return res.status(404).json({ message: "Session unavailable" });
    const room = `session:${session._id}`;
    io.to(room).emit("sessionRevoked");
    io.in(room).disconnectSockets(true);
    res.json({ current: session._id === req.sessionId });
  } catch { res.status(503).json({ message: "Could not sign out that session" }); }
});
router.put("/update-profile", protectRoute, updateProfile);
router.put("/password", protectRoute, passwordRateLimit, changePassword);

// Binds the first public key to an account, optionally with its encrypted
// backup. An existing key is never replaced here; rotation requires the
// current password (see /encryption-key/rotate).
router.put("/encryption-key", protectRoute, async (req, res) => {
  const { publicKey, backup, recoveryVerifier } = req.body;
  if (!validPublicKey(publicKey))
    return res.status(400).json({ message: "Invalid P-256 public key" });
  const parsed = backup === undefined ? undefined : parseKeyBackup(backup, publicKey);
  if (parsed === null || (parsed?.version === RECOVERY_BACKUP_VERSION) !== validVerifier(recoveryVerifier))
    return res.status(400).json({ message: "Invalid encrypted key backup" });
  try {
    // A backup may only accompany the very first key. Afterwards it changes
    // only through password-checked rotation or password change, so a stolen
    // session cookie cannot overwrite a working backup.
    const user = await User.findOneAndUpdate(
      parsed
        ? { _id: req.user._id, encryptionKey: null }
        : { _id: req.user._id, $or: [{ encryptionKey: null }, { encryptionKey: publicKey }] },
      { $set: { encryptionKey: publicKey, ...(parsed ? { keyBackup: parsed, recoveryVerifier: recoveryVerifier ?? null } : {}) } },
      { new: true },
    ).select("_id fullName email profilePic encryptionKey");
    if (!user)
      return res.status(409).json({
        message:
          "This account already uses a different encryption key. Unlock it with your password.",
      });
    res.json(user);
  } catch {
    res.status(503).json({ message: "Could not save the encryption key" });
  }
});

// Returns the password-encrypted private key for a new device to unlock locally.
router.get("/key-backup", protectRoute, async (req, res) => {
  try {
    const user = await User.findById(req.user._id).select("encryptionKey +keyBackup").lean();
    const backup = user?.keyBackup?.publicKey === user?.encryptionKey ? publicBackup(user?.keyBackup) : null;
    res.json({ backup });
  } catch {
    res.status(503).json({ message: "Could not load the encrypted key backup" });
  }
});

// Replaces the account key with a new one and its backup. Used to turn on
// multi-device access for keys created before backups existed, or to reset a
// key whose only copy was lost. Contacts see a key change and must re-verify.
router.post("/encryption-key/rotate", protectRoute, passwordRateLimit, async (req, res) => {
  const { currentPassword, publicKey, backup, previousKey, recoveryVerifier } = req.body;
  if (typeof currentPassword !== "string" || Buffer.byteLength(currentPassword, "utf8") > 72)
    return res.status(400).json({ message: "Current password is incorrect" });
  const parsed = validPublicKey(publicKey) ? parseKeyBackup(backup, publicKey) : null;
  if (!parsed || (parsed.version === RECOVERY_BACKUP_VERSION) !== validVerifier(recoveryVerifier))
    return res.status(400).json({ message: "Invalid encrypted key backup" });
  try {
    const user = await User.findById(req.user._id).select("password encryptionKey");
    if (!user || !(await bcrypt.compare(currentPassword, user.password)))
      return res.status(400).json({ message: "Current password is incorrect" });
    // previousKey guards against two devices rotating concurrently.
    if ((previousKey ?? null) !== (user.encryptionKey ?? null))
      return res.status(409).json({ message: "Your encryption key changed on another device. Sign in again." });
    const updated = await User.findOneAndUpdate(
      { _id: user._id, encryptionKey: user.encryptionKey ?? null },
      { $set: { encryptionKey: publicKey, keyBackup: parsed, recoveryVerifier: recoveryVerifier ?? null } },
      { new: true },
    ).select("_id fullName email profilePic encryptionKey");
    if (!updated)
      return res.status(409).json({ message: "Your encryption key changed on another device. Sign in again." });
    res.json(updated);
  } catch {
    res.status(503).json({ message: "Could not replace the encryption key" });
  }
});

// Sets up or replaces the recovery key for the current encryption key. The
// password is required so a stolen session cookie cannot swap it.
router.put("/recovery", protectRoute, passwordRateLimit, async (req, res) => {
  const { currentPassword, backup, recoveryVerifier } = req.body;
  if (typeof currentPassword !== "string" || Buffer.byteLength(currentPassword, "utf8") > 72)
    return res.status(400).json({ message: "Current password is incorrect" });
  try {
    const user = await User.findById(req.user._id).select("password encryptionKey");
    if (!user || !(await bcrypt.compare(currentPassword, user.password)))
      return res.status(400).json({ message: "Current password is incorrect" });
    const parsed = user.encryptionKey ? parseKeyBackup(backup, user.encryptionKey) : null;
    if (parsed?.version !== RECOVERY_BACKUP_VERSION || !validVerifier(recoveryVerifier))
      return res.status(400).json({ message: "Invalid recovery key backup" });
    const updated = await User.updateOne(
      { _id: user._id, encryptionKey: user.encryptionKey },
      { $set: { keyBackup: parsed, recoveryVerifier } },
    );
    if (!updated.modifiedCount && !updated.matchedCount)
      return res.status(409).json({ message: "Your encryption key changed on another device. Sign in again." });
    res.json({ ok: true });
  } catch {
    res.status(503).json({ message: "Could not save the recovery key" });
  }
});

// Forgotten password: the recovery key proves ownership without email. The
// server checks SHA-256(proof) against the stored verifier, sets the new
// password, signs out every other session and signs this one in.
router.post("/recover", loginRateLimit, async (req, res) => {
  const { email, proof, newPassword } = req.body;
  const mismatch = () => res.status(400).json({ message: "That email and recovery key do not match." });
  if (typeof email !== "string" || email.length > 254) return mismatch();
  if (typeof newPassword !== "string" || newPassword.length < 10 || Buffer.byteLength(newPassword, "utf8") > 72)
    return res.status(400).json({ message: "Password must be at least 10 characters and at most 72 UTF-8 bytes" });
  try {
    const user = await User.findOne({ email }).select("+recoveryVerifier password sessionVersion");
    // Same work and message whether or not the account exists.
    const valid = proofMatches(proof, user?.recoveryVerifier || "0".repeat(64)) && Boolean(user?.recoveryVerifier);
    if (!valid) return mismatch();
    const password = await bcrypt.hash(newPassword, 12);
    const changed = await User.findOneAndUpdate(
      { _id: user._id, password: user.password },
      { $set: { password }, $inc: { sessionVersion: 1 } },
      { new: true },
    ).select("_id fullName email profilePic encryptionKey sessionVersion");
    if (!changed) return res.status(409).json({ message: "Please try again." });
    io.to(String(changed._id)).emit("sessionRevoked");
    io.in(String(changed._id)).disconnectSockets(true);
    await generateToken(changed._id, res, changed.sessionVersion || 0);
    res.json({ _id: changed._id, fullName: changed.fullName, email: changed.email, profilePic: changed.profilePic, encryptionKey: changed.encryptionKey });
  } catch {
    res.status(503).json({ message: "Could not reset the password" });
  }
});

router.get("/check", protectRoute, checkAuth);

export default router;
