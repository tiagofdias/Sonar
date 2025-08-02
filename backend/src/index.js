import express from "express";
import dotenv from "dotenv";
import cookieParser from "cookie-parser";
import cors from "cors";

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
  // Serve static files from the React app build directory
  const frontendPath = path.join(__dirname, "../../frontend/dist");
  console.log("Serving static files from:", frontendPath);
  
  app.use(express.static(frontendPath));

  // Catch all handler: send back React's index.html file for any non-API routes
  app.get("*", (req, res) => {
    const indexPath = path.join(frontendPath, "index.html");
    console.log("Serving index.html from:", indexPath);
    res.sendFile(indexPath);
  });
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
