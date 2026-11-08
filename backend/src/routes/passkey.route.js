import express from "express";
import mongoose from "mongoose";
import bcrypt from "bcryptjs";
import { randomBytes, createHash } from "node:crypto";
import {
  generateRegistrationOptions,
  verifyRegistrationResponse,
  generateAuthenticationOptions,
  verifyAuthenticationResponse,
} from "@simplewebauthn/server";
import Passkey from "../models/passkey.model.js";
import Challenge from "../models/passkeyChallenge.model.js";
import User from "../models/user.model.js";
import { protectRoute } from "../middleware/auth.middleware.js";
import { passwordRateLimit } from "../middleware/passwordRateLimit.js";
import { createAuthRateLimit } from "../middleware/authRateLimit.js";
import { selectableRateLimit } from "../middleware/sharedRateLimit.js";
import { generateToken } from "../lib/utils.js";
import { io } from "../lib/socket.js";
const router = express.Router();
const digest = (text) => createHash("sha256").update(text).digest("hex");
const safe = (handler) => async (req, res) => {
  try {
    await handler(req, res);
  } catch {
    res
      .status(400)
      .json({ message: "Passkey verification failed. Please try again." });
  }
};
const cookieName = (purpose) => `sonar-passkey-${purpose}`;
const cookieOptions = () => ({
  httpOnly: true,
  sameSite: "strict",
  secure: process.env.NODE_ENV === "production",
  path: "/api/auth/passkeys",
});
const publicPasskey = (key) => ({
  _id: key._id,
  name: key.name,
  createdAt: key.createdAt,
  lastUsedAt: key.lastUsedAt,
  rpID: key.rpID,
});
const sessionGuard = (user) =>
  (user.sessionVersion || 0) === 0
    ? { $or: [{ sessionVersion: 0 }, { sessionVersion: { $exists: false } }] }
    : { sessionVersion: user.sessionVersion };
async function touchAccount(user, session, revoke = false) {
  const current = await User.findOneAndUpdate(
    { _id: user._id, ...sessionGuard(user) },
    { $inc: { passkeyRevision: 1, ...(revoke ? { sessionVersion: 1 } : {}) } },
    { session, new: true },
  );
  if (!current) throw new Error("Account security changed");
}
function relyingParty() {
  const url = new URL(
    process.env.WEBAUTHN_ORIGIN ||
      (process.env.NODE_ENV === "production"
        ? process.env.FRONTEND_URL
        : "http://localhost:5173"),
  );
  if (
    (url.protocol !== "https:" &&
      !(url.protocol === "http:" && url.hostname === "localhost")) ||
    /^[\d.]+$/.test(url.hostname) ||
    url.hostname.includes(":")
  )
    throw new Error("Passkeys require HTTPS or localhost");
  return { origin: url.origin, rpID: url.hostname };
}
async function issueChallenge(res, purpose, options, extra = {}) {
  const token = randomBytes(32).toString("base64url");
  await Challenge.create({
    tokenHash: digest(token),
    challenge: options.challenge,
    purpose,
    ...relyingParty(),
    ...extra,
    expiresAt: new Date(Date.now() + 5 * 60 * 1000),
  });
  res.cookie(cookieName(purpose), token, {
    ...cookieOptions(),
    maxAge: 5 * 60 * 1000,
  });
  res.json({ options, origin: relyingParty().origin });
}
async function consumeChallenge(req, res, purpose, extra = {}) {
  const token = req.cookies[cookieName(purpose)];
  res.clearCookie(cookieName(purpose), cookieOptions());
  if (typeof token !== "string") return null;
  return Challenge.findOneAndDelete({
    tokenHash: digest(token),
    purpose,
    expiresAt: { $gt: new Date() },
    ...extra,
  });
}
async function checkPassword(req) {
  if (
    typeof req.body.currentPassword !== "string" ||
    Buffer.byteLength(req.body.currentPassword, "utf8") > 72
  )
    return false;
  const user = await User.findById(req.user._id);
  return (
    user &&
    (user.sessionVersion || 0) === (req.user.sessionVersion || 0) &&
    (await bcrypt.compare(req.body.currentPassword, user.password))
  );
}
const passkeyOptionsLimit = selectableRateLimit({ scope: "passkey-options", limit: 30, local: createAuthRateLimit({ limit: 30 }) });
const passkeyVerifyLimit = selectableRateLimit({ scope: "passkey-verify", limit: 30, local: createAuthRateLimit({ limit: 30 }) });
const loginAttempts = new Map();
router.use((req, res, next) => {
  const now = Date.now(),
    key = req.ip;
  let entry = loginAttempts.get(key);
  if (!entry || entry.expiresAt < now) {
    if (loginAttempts.size >= 10000)
      for (const [id, item] of loginAttempts)
        if (item.expiresAt < now) loginAttempts.delete(id);
    if (loginAttempts.size >= 10000)
      return res.status(503).json({ message: "Please try again later" });
    entry = { count: 0, expiresAt: now + 5 * 60 * 1000 };
    loginAttempts.set(key, entry);
  }
  if (++entry.count > 100)
    return res
      .status(429)
      .json({
        message: "Too many passkey requests. Please wait a few minutes.",
      });
  next();
});
router.get(
  "/",
  protectRoute,
  safe(async (req, res) => {
    res.json(
      (await Passkey.find({ userId: req.user._id }).sort({ createdAt: 1 })).map(
        publicPasskey,
      ),
    );
  }),
);
router.post(
  "/register/options",
  protectRoute,
  passwordRateLimit,
  safe(async (req, res) => {
    if (!(await checkPassword(req)))
      return res.status(400).json({ message: "Current password is incorrect" });
    const keys = await Passkey.find({ userId: req.user._id });
    if (keys.length >= 10)
      return res
        .status(400)
        .json({ message: "You can register up to 10 passkeys" });
    const name = typeof req.body.name === "string" ? req.body.name.trim() : "";
    if (!name || name.length > 60)
      return res
        .status(400)
        .json({ message: "Give your passkey a name of 1-60 characters" });
    const { rpID } = relyingParty();
    const options = await generateRegistrationOptions({
      rpName: "Sonar",
      rpID,
      userID: new TextEncoder().encode(String(req.user._id)),
      userName: req.user.email,
      userDisplayName: req.user.fullName,
      attestationType: "none",
      excludeCredentials: keys
        .filter((k) => k.rpID === rpID)
        .map((k) => ({ id: k.credentialId, transports: k.transports })),
      authenticatorSelection: {
        residentKey: "required",
        userVerification: "required",
      },
      supportedAlgorithmIDs: [-7, -257],
      timeout: 120000,
    });
    await issueChallenge(res, "registration", options, {
      userId: req.user._id,
      sessionVersion: req.user.sessionVersion || 0,
      name,
    });
  }),
);
router.post(
  "/register/verify",
  protectRoute,
  safe(async (req, res) => {
    const challenge = await consumeChallenge(req, res, "registration", {
      userId: req.user._id,
      sessionVersion: req.user.sessionVersion || 0,
    });
    if (!challenge)
      return res
        .status(400)
        .json({
          message: "Passkey request expired or was already used. Start again.",
        });
    const result = await verifyRegistrationResponse({
      response: req.body.response,
      expectedChallenge: challenge.challenge,
      expectedOrigin: challenge.origin,
      expectedRPID: challenge.rpID,
      requireUserVerification: true,
      supportedAlgorithmIDs: [-7, -257],
    });
    if (!result.verified || !result.registrationInfo)
      return res.status(400).json({ message: "Passkey could not be verified" });
    const { credential } = result.registrationInfo;
    let key;
    await mongoose.connection.transaction(async (session) => {
      await touchAccount(req.user, session);
      if (
        (await Passkey.countDocuments({ userId: req.user._id }).session(
          session,
        )) >= 10
      )
        throw new Error("Passkey limit reached");
      [key] = await Passkey.create(
        [
          {
            userId: req.user._id,
            credentialId: credential.id,
            publicKey: Buffer.from(credential.publicKey),
            counter: credential.counter,
            transports: credential.transports || [],
            name: challenge.name,
            rpID: challenge.rpID,
          },
        ],
        { session },
      );
    });
    res.status(201).json(publicPasskey(key));
  }),
);
router.post(
  "/login/options",
  passkeyOptionsLimit,
  safe(async (_req, res) => {
    const options = await generateAuthenticationOptions({
      rpID: relyingParty().rpID,
      userVerification: "required",
      timeout: 120000,
    });
    await issueChallenge(res, "authentication", options);
  }),
);
router.post(
  "/login/verify",
  passkeyVerifyLimit,
  safe(async (req, res) => {
    const challenge = await consumeChallenge(req, res, "authentication");
    if (!challenge)
      return res
        .status(400)
        .json({
          message: "Passkey request expired or was already used. Start again.",
        });
    const response = req.body.response;
    if (!response || typeof response.id !== "string")
      return res.status(400).json({ message: "Invalid passkey response" });
    const key = await Passkey.findOne({
      credentialId: response.id,
      rpID: challenge.rpID,
    }).select("+publicKey");
    if (!key)
      return res.status(400).json({ message: "Passkey could not be verified" });
    const user = await User.findById(key.userId).select("-password");
    if (!user)
      return res.status(400).json({ message: "Passkey could not be verified" });
    if (
      response.response?.userHandle !==
      Buffer.from(String(user._id)).toString("base64url")
    )
      return res
        .status(400)
        .json({ message: "Passkey account does not match" });
    const result = await verifyAuthenticationResponse({
      response,
      expectedChallenge: challenge.challenge,
      expectedOrigin: challenge.origin,
      expectedRPID: challenge.rpID,
      credential: {
        id: key.credentialId,
        publicKey: new Uint8Array(key.publicKey),
        counter: key.counter,
        transports: key.transports,
      },
      requireUserVerification: true,
    });
    if (!result.verified)
      return res.status(400).json({ message: "Passkey could not be verified" });
    await mongoose.connection.transaction(async (session) => {
      await touchAccount(user, session);
      const updated = await Passkey.updateOne(
        { _id: key._id, counter: key.counter },
        {
          counter: result.authenticationInfo.newCounter,
          lastUsedAt: new Date(),
        },
        { session },
      );
      if (!updated.matchedCount) throw new Error("Passkey changed");
    });
    await generateToken(user._id, res, user.sessionVersion || 0, "passkey");
    res.json({
      _id: user._id,
      fullName: user.fullName,
      email: user.email,
      profilePic: user.profilePic,
      encryptionKey: user.encryptionKey,
    });
  }),
);
router.delete(
  "/:id",
  protectRoute,
  passwordRateLimit,
  safe(async (req, res) => {
    if (!(await checkPassword(req)))
      return res.status(400).json({ message: "Current password is incorrect" });
    if (!/^[a-f0-9]{24}$/i.test(req.params.id))
      return res.status(404).json({ message: "Passkey unavailable" });
    if (!(await Passkey.exists({ _id: req.params.id, userId: req.user._id })))
      return res.status(404).json({ message: "Passkey unavailable" });
    await mongoose.connection.transaction(async (session) => {
      await touchAccount(req.user, session, true);
      const deleted = await Passkey.findOneAndDelete(
        { _id: req.params.id, userId: req.user._id },
        { session },
      );
      if (!deleted) throw new Error("Passkey changed");
    });
    io.to(String(req.user._id)).emit("sessionRevoked");
    io.in(String(req.user._id)).disconnectSockets(true);
    res.clearCookie("jwt", {
      httpOnly: true,
      sameSite: "strict",
      secure: process.env.NODE_ENV === "production",
    });
    res.json({ message: "Passkey removed. Sign in again." });
  }),
);
export default router;
