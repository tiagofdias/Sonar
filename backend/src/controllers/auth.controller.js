import { generateToken } from "../lib/utils.js";
import User from "../models/user.model.js";
import bcrypt from "bcryptjs";
import { io } from "../lib/socket.js";
import { parseKeyBackup } from "../lib/keyBackup.js";
import { availableUsername, normalizeUsername, USERNAME_RULES } from "../lib/usernames.js";
const publicUser = (user) => ({
  _id: user._id,
  fullName: user.fullName,
  username: user.username,
  email: user.email,
  profilePic: user.profilePic,
  encryptionKey: user.encryptionKey,
});
const clearCookie = (res) =>
  res.clearCookie("jwt", {
    httpOnly: true,
    sameSite: "strict",
    secure: process.env.NODE_ENV === "production",
  });
const revokeSockets = (userId) => {
  io.to(String(userId)).emit("sessionRevoked");
  io.in(String(userId)).disconnectSockets(true);
};

export const signup = async (req, res) => {
  const { fullName, email, password, username: requested } = req.body;
  try {
    if (
      typeof fullName !== "string" ||
      !fullName.trim() ||
      fullName.length > 80 ||
      typeof email !== "string" ||
      !email.includes("@") ||
      email.length > 254 ||
      typeof password !== "string"
    ) {
      return res.status(400).json({ message: "All fields are required" });
    }

    if (password.length < 10 || Buffer.byteLength(password, "utf8") > 72) {
      return res
        .status(400)
        .json({
          message:
            "Password must be at least 10 characters and at most 72 UTF-8 bytes",
        });
    }

    const user = await User.findOne({ email });

    if (user) return res.status(400).json({ message: "Email already exists" });

    let username;
    if (requested) {
      username = normalizeUsername(requested);
      if (!username) return res.status(400).json({ message: USERNAME_RULES });
      if (await User.exists({ username })) return res.status(400).json({ message: "That username is taken" });
    } else {
      username = await availableUsername(fullName || email);
    }

    const salt = await bcrypt.genSalt(10);
    const hashedPassword = await bcrypt.hash(password, salt);

    const newUser = new User({
      fullName: fullName.trim(),
      username,
      email,
      password: hashedPassword,
    });

    if (newUser) {
      // generate jwt token here
      await newUser.save();
      await generateToken(newUser._id, res);

      res.status(201).json(publicUser(newUser));
    } else {
      res.status(400).json({ message: "Invalid user data" });
    }
  } catch (error) {
    if (error.code === 11000) return res.status(400).json({ message: "That email or username is already taken" });
    console.log("Error in signup controller", error.message);
    res.status(500).json({ message: "Internal Server Error" });
  }
};

export const login = async (req, res) => {
  const { email, password } = req.body;
  try {
    if (
      typeof email !== "string" ||
      typeof password !== "string" ||
      email.length > 254 ||
      Buffer.byteLength(password, "utf8") > 72
    )
      return res.status(400).json({ message: "Invalid credentials" });
    const user = await User.findOne({ email });

    if (!user) {
      return res.status(400).json({ message: "Invalid credentials" });
    }

    const isPasswordCorrect = await bcrypt.compare(password, user.password);
    if (!isPasswordCorrect) {
      return res.status(400).json({ message: "Invalid credentials" });
    }

    await generateToken(user._id, res, user.sessionVersion || 0);

    res.status(200).json(publicUser(user));
  } catch (error) {
    console.log("Error in login controller", error.message);
    res.status(500).json({ message: "Internal Server Error" });
  }
};

export const logout = async (req, res) => {
  try {
    await User.updateOne(
      { _id: req.user._id },
      { $inc: { sessionVersion: 1 } },
    );
    revokeSockets(req.user._id);
    clearCookie(res);
    res.status(200).json({ message: "Logged out successfully" });
  } catch (error) {
    console.log("Error in logout controller", error.message);
    res.status(500).json({ message: "Internal Server Error" });
  }
};

export const updateProfile = async (req, res) => {
  const { fullName, username: requested } = req.body;
  if (typeof fullName !== "string" || !fullName.trim() || fullName.length > 80)
    return res
      .status(400)
      .json({ message: "Display name must be between 1 and 80 characters" });
  const username = requested === undefined ? undefined : normalizeUsername(requested);
  if (username === null) return res.status(400).json({ message: USERNAME_RULES });
  try {
    const user = await User.findByIdAndUpdate(
      req.user._id,
      { fullName: fullName.trim(), ...(username ? { username } : {}) },
      { new: true },
    ).select("-password");
    res.json(publicUser(user));
  } catch (error) {
    if (error.code === 11000) return res.status(409).json({ message: "That username is taken" });
    res.status(500).json({ message: "Could not update profile" });
  }
};

export const checkAuth = (req, res) => {
  try {
    res.status(200).json(publicUser(req.user));
  } catch (error) {
    console.log("Error in checkAuth controller", error.message);
    res.status(500).json({ message: "Internal Server Error" });
  }
};

export const changePassword = async (req, res) => {
  const { currentPassword, newPassword } = req.body;
  if (
    typeof currentPassword !== "string" ||
    typeof newPassword !== "string" ||
    newPassword.length < 10 ||
    Buffer.byteLength(newPassword, "utf8") > 72
  )
    return res
      .status(400)
      .json({
        message:
          "Use a new password of at least 10 characters and at most 72 UTF-8 bytes",
      });
  if (currentPassword === newPassword)
    return res.status(400).json({ message: "Choose a different password" });
  try {
    const user = await User.findById(req.user._id).select("+keyBackup");
    if (!user || !(await bcrypt.compare(currentPassword, user.password)))
      return res.status(400).json({ message: "Current password is incorrect" });
    // The encrypted key backup is wrapped with the password, so it must be
    // re-wrapped by the client in the same change or it would become unusable.
    let keyBackup;
    if (user.keyBackup?.ciphertext && user.keyBackup.version === 1 && user.keyBackup.publicKey === user.encryptionKey) {
      keyBackup = parseKeyBackup(req.body.keyBackup, user.encryptionKey);
      if (!keyBackup)
        return res.status(400).json({ message: "Update Sonar and try again: your encrypted key must be re-protected with the new password." });
    }
    const password = await bcrypt.hash(newPassword, 12);
    const changed = await User.findOneAndUpdate(
      { _id: user._id, password: user.password, encryptionKey: user.encryptionKey ?? null },
      { $set: { password, ...(keyBackup ? { keyBackup } : {}) }, $inc: { sessionVersion: 1 } },
      { new: true },
    );
    if (!changed)
      return res
        .status(409)
        .json({
          message: "Password changed in another session. Sign in again.",
        });
    clearCookie(res);
    revokeSockets(user._id);
    res.json({
      message: "Password updated. Sign in again with your new password.",
    });
  } catch {
    res.status(500).json({ message: "Could not change password" });
  }
};
