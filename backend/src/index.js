import express from "express";
import dotenv from "dotenv";
import cookieParser from "cookie-parser";
import cors from "cors";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

import { connectDB } from "./lib/db.js";

import authRoutes from "./routes/auth.route.js";
import messageRoutes from "./routes/message.route.js";
import { app, server } from "./lib/socket.js";

dotenv.config();

const PORT = process.env.PORT || 5000;
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Get allowed origins based on environment
const getAllowedOrigins = () => {
  if (process.env.NODE_ENV === "production") {
    return [
      process.env.FRONTEND_URL,
      /^https:\/\/.*\.onrender\.com$/
    ].filter(Boolean);
  }
  return ["http://localhost:5173", "http://localhost:3000"];
};

app.use(express.json());
app.use(cookieParser());
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

if (process.env.NODE_ENV === "production") {
  // Serve the static frontend build - find the correct path based on Render's layout
  const candidates = [
    path.resolve(__dirname, "../dist"),                    // backend/dist (copied approach)
    path.resolve(__dirname, "../../frontend/dist"),        // from backend/src to frontend/dist  
    path.resolve(process.cwd(), "../frontend/dist"),       // from src/backend to frontend/dist
    "/opt/render/project/frontend/dist",                   // Render absolute path
    "/opt/render/project/dist"                             // Root dist folder
  ];
  
  let frontendPath = null;
  console.log("🔍 Searching for frontend build...");
  console.log("Current working directory:", process.cwd());
  console.log("__dirname:", __dirname);
  
  for (const candidate of candidates) {
    console.log("Checking:", candidate);
    if (fs.existsSync(candidate)) {
      frontendPath = candidate;
      console.log("✅ Found frontend build at:", frontendPath);
      break;
    }
  }
  
  if (frontendPath) {
    app.use(express.static(frontendPath));
    console.log("✅ Serving static files from:", frontendPath);

    // Catch all handler: send back React's index.html file for any non-API routes
    app.get("*", (req, res) => {
      const indexPath = path.join(frontendPath, "index.html");
      res.sendFile(indexPath);
    });
  } else {
    console.log("❌ No frontend build found in any candidate paths");
    
    app.get("*", (req, res) => {
      res.status(404).json({
        error: "Frontend build not found",
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

server.listen(PORT, () => {
  console.log("server is running on PORT:" + PORT);
  connectDB();
});
