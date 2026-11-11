import jwt from "jsonwebtoken";
import User from "../models/user.model.js";
import { validSession, sessionHash } from "../lib/sessions.js";

export const protectRoute = async (req, res, next) => {
  try {
    const token = req.cookies.jwt;

    if (!token) {
      return res
        .status(401)
        .json({ message: "Unauthorized - No Token Provided" });
    }

    const decoded = jwt.verify(token, process.env.JWT_SECRET, { algorithms: ["HS256"] });

    if (!decoded) {
      return res.status(401).json({ message: "Unauthorized - Invalid Token" });
    }

    const user = await User.findById(decoded.userId).select("-password");

    if (!user) {
      return res.status(404).json({ message: "User not found" });
    }
    if ((decoded.sessionVersion || 0) !== (user.sessionVersion || 0))
      return res
        .status(401)
        .json({ message: "Session expired. Please sign in again." });

    if (!(await validSession(decoded)))
      return res.status(401).json({ message: "Session expired. Please sign in again." });
    req.user = user;
    req.sessionId = decoded.sessionId ? sessionHash(decoded.sessionId) : null;

    next();
  } catch (error) {
    if (
      error.name === "JsonWebTokenError" ||
      error.name === "TokenExpiredError"
    )
      return res
        .status(401)
        .json({ message: "Session expired. Please sign in again." });
    console.log("Error in protectRoute middleware: ", error.message);
    res.status(500).json({ message: "Internal server error" });
  }
};
