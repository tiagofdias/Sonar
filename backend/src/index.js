import express from "express";
import dotenv from "dotenv";
import cookieParser from "cookie-parser";
import cors from "cors";
import fs from "fs";
import path from "path";

import { connectDB } from "./lib/db.js";

import authRoutes from "./routes/auth.route.js";
import messageRoutes from "./routes/message.route.js";
import { app, server } from "./lib/socket.js";

dotenv.config();

const PORT = process.env.PORT || 5000;
const __dirname = path.resolve();

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
  // Based on Render's directory structure: 
  // cwd: /opt/render/project/src/backend
  // We need: /opt/render/project/frontend/dist
  const frontendPath = path.resolve(process.cwd(), "../../frontend/dist");
  
  console.log("Looking for frontend files at:", frontendPath);
  console.log("Current working directory:", process.cwd());
  console.log("Resolved path:", path.resolve(process.cwd(), "../../frontend/dist"));
  
  if (fs.existsSync(frontendPath)) {
    console.log("✅ Found frontend files! Serving static files from:", frontendPath);
    app.use(express.static(frontendPath));

    // Catch all handler: send back React's index.html file for any non-API routes
    app.get("*", (req, res) => {
      const indexPath = path.join(frontendPath, "index.html");
      if (fs.existsSync(indexPath)) {
        console.log("✅ Serving index.html from:", indexPath);
        res.sendFile(indexPath);
      } else {
        console.log("❌ index.html not found at:", indexPath);
        res.status(404).send("index.html not found");
      }
    });
  } else {
    console.log("❌ Frontend build directory not found at:", frontendPath);
    
    // Debug: Check what exists in the project
    const projectRoot = "/opt/render/project";
    console.log("📁 Project root contents:");
    try {
      const rootContents = fs.readdirSync(projectRoot);
      console.log(rootContents);
      
      if (rootContents.includes("frontend")) {
        console.log("📁 Frontend directory contents:");
        const frontendContents = fs.readdirSync(path.join(projectRoot, "frontend"));
        console.log(frontendContents);
      }
    } catch (err) {
      console.log("Error reading directories:", err.message);
    }
    
    app.get("*", (req, res) => {
      res.status(404).json({
        error: "Frontend build not found",
        expectedPath: frontendPath,
        resolvedPath: path.resolve(process.cwd(), "../../frontend/dist"),
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
