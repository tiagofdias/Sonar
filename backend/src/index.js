import express from "express";
import dotenv from "dotenv";
import cookieParser from "cookie-parser";
import cors from "cors";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

import { connectDB } from "./lib/db.js";
import { ensureMessageRetentionIndexes } from "./lib/retentionIndexes.js";
import { ensureUsernames } from "./lib/usernames.js";
import mongoose from "mongoose";

import authRoutes from "./routes/auth.route.js";
import messageRoutes from "./routes/message.route.js";
import communityRoutes from "./routes/community.route.js";
import voiceRoutes from "./routes/voice.route.js";
import friendsRoutes from "./routes/friends.route.js";
import preferencesRoutes from "./routes/preferences.route.js";
import { seedFriendsFromMessages } from "./lib/friends.js";
import attachmentRoutes from "./routes/attachment.route.js";
import Attachment from "./models/attachment.model.js";
import { createMatrixDeviceRouter } from "./routes/matrixDevice.route.js";
import { createMatrixMessagingRouter } from "./routes/matrixMessaging.route.js";
import { app, server, allowedOrigins, io } from "./lib/socket.js";

dotenv.config();

const PORT = process.env.PORT || 5000;
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Only list proxy addresses controlled by the operator; never trust arbitrary forwarded headers.
if (process.env.TRUST_PROXY_CIDRS) {
  const proxies = process.env.TRUST_PROXY_CIDRS.split(",").map(value => value.trim()).filter(Boolean);
  if (!proxies.length || proxies.some(value => ["true", "false", "*"].includes(value.toLowerCase())))
    throw new Error("TRUST_PROXY_CIDRS must list explicit proxy IP addresses or CIDR ranges");
  app.set("trust proxy", proxies);
}

// Get allowed origins based on environment
const getAllowedOrigins = () => {
  if (process.env.NODE_ENV === "production") {
    return [
      process.env.FRONTEND_URL,
    ].filter(Boolean);
  }
  return allowedOrigins;
};

app.use(express.json({ limit: "800kb" }));
app.use("/api", (req, res, next) => { res.set("Cache-Control", "no-store"); next(); });
app.use(cookieParser());
app.use((req, res, next) => {
  if (!["GET", "HEAD", "OPTIONS"].includes(req.method) && req.headers.origin) {
    const localOrigin = `${req.secure ? "https" : "http"}://${req.headers.host}`;
    // Hosting proxies (Render, Fly, etc.) terminate TLS, so Express sees plain
    // HTTP while the browser reports an https:// origin for the same host.
    const proxiedOrigin = process.env.NODE_ENV === "production" ? `https://${req.headers.host}` : null;
    const publicOrigin = process.env.FRONTEND_URL;
    if (req.headers.origin !== localOrigin && req.headers.origin !== proxiedOrigin && req.headers.origin !== publicOrigin && !allowedOrigins.includes(req.headers.origin)) {
      return res.status(403).json({ message: "Origin not allowed" });
    }
  }
  res.set("X-Content-Type-Options", "nosniff");
  res.set("Referrer-Policy", "no-referrer");
  next();
});
app.use(
  cors({
    origin: getAllowedOrigins(),
    credentials: true,
  })
);

// Health check endpoint
app.get("/health", (req, res) => {
  res.json({
    status: "OK",
    environment: process.env.NODE_ENV,
    timestamp: new Date().toISOString()
  });
});

app.use("/api/auth", authRoutes);
app.use("/api/messages", messageRoutes);
app.use("/api/friends", friendsRoutes);
app.use("/api/preferences", preferencesRoutes);
app.use("/api/communities", communityRoutes);
app.use("/api/voice", voiceRoutes);
app.use("/api/attachments", attachmentRoutes);
// Experimental device registration is off by default and requires a stable,
// operator-configured identity domain. Existing legacy keys are never changed.
if (process.env.SONAR_MATRIX_EXPERIMENTAL === "true") {
  app.use("/api/matrix/device", createMatrixDeviceRouter(process.env.MATRIX_SERVER_NAME));
  app.use("/api/matrix/messages", createMatrixMessagingRouter(process.env.MATRIX_SERVER_NAME, { notify: members => {
    for (const member of members) {
      const account = /^@([a-f0-9]{24}):[a-z0-9.-]{1,200}$/.exec(member)?.[1];
      if (account) io.to(account).emit("matrixRoomChanged");
    }
  } }));
}

if (process.env.NODE_ENV === "production") {
  // Serve the static frontend build
  // Priority order for finding the frontend build
  const candidates = [
    path.resolve(__dirname, "../dist"),                    // backend/src to backend/dist (primary for Render)
    path.resolve(__dirname, "../../dist"),                 // fallback: project root dist
    path.resolve(__dirname, "../../frontend/dist"),     // fallback: from backend/src to frontend/dist
  ];

  let frontendPath = null;
  console.log(" Searching for frontend build...");
  console.log("Current working directory:", process.cwd());
  console.log("__dirname:", __dirname);

  for (const candidate of candidates) {
    console.log("Checking:", candidate);
    if (fs.existsSync(candidate)) {
      const indexFile = path.join(candidate, "index.html");
      if (fs.existsSync(indexFile)) {
        frontendPath = candidate;
        console.log(" Found frontend build at:", frontendPath);
        break;
      }
    }
  }

  if (frontendPath) {
    app.use(express.static(frontendPath));
    console.log(" Serving static files from:", frontendPath);

    // Catch all handler: send back React\'s index.html file for any non-API routes
    app.get("*", (req, res) => {
      const indexPath = path.join(frontendPath, "index.html");
      res.sendFile(indexPath);
    });
  } else {
    console.log(" No frontend build found in any candidate paths");
    console.log("Searched paths:", candidates);

    app.get("*", (req, res) => {
      res.status(404).json({
        error: "Frontend build not found",
        message: "Please ensure the frontend is built and copied to the correct location",
        candidates: candidates,
        cwd: process.cwd(),
        __dirname: __dirname
      });
    });
  }
} else {
  // Development route
  app.get("/", (req, res) => {
    res.json({ message: "API is running in development mode" });
  });
}

try {
  if (!process.env.JWT_SECRET || process.env.JWT_SECRET.length < 32) throw new Error("JWT_SECRET must contain at least 32 characters");
  await connectDB();
  await ensureMessageRetentionIndexes({ matrix: process.env.SONAR_MATRIX_EXPERIMENTAL === "true" });
  await Attachment.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 0 });
  await ensureUsernames();
  await seedFriendsFromMessages();
  server.listen(PORT, () => console.log(`Sonar listening on port ${PORT}`));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
  server.close();
  await mongoose.disconnect();
}
