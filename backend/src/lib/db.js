import mongoose from "mongoose";
import dns from "node:dns";
export const connectDB = async () => {
  if (!process.env.MONGODB_URI) throw new Error("MONGODB_URI is required. See backend/.env.example.");
  if (process.env.MONGODB_DNS_SERVERS) {
    dns.setServers(process.env.MONGODB_DNS_SERVERS.split(",").map(value => value.trim()));
  }
  await mongoose.connect(process.env.MONGODB_URI, { serverSelectionTimeoutMS: 15000 });
  console.log("MongoDB connected");
};
