import jwt from "jsonwebtoken";
import { createSession, SESSION_MS } from "./sessions.js";

export const generateToken = async (userId, res, sessionVersion = 0, method = "password") => {
  const sessionId = await createSession(userId, sessionVersion, method);
  const token = jwt.sign({ userId, sessionVersion, sessionId }, process.env.JWT_SECRET, {
    expiresIn: "7d",
  });

  res.cookie("jwt", token, {
    maxAge: SESSION_MS, // MS
    httpOnly: true, // prevent XSS attacks cross-site scripting attacks
    sameSite: "strict", // CSRF attacks cross-site request forgery attacks
    secure: process.env.NODE_ENV === "production",
  });

  return token;
};
