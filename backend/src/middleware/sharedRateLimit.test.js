import { test } from "node:test";
import assert from "node:assert/strict";
import mongoose from "mongoose";
import express from "express";
import cookieParser from "cookie-parser";
import authRoutes from "../routes/auth.route.js";
import { MongoMemoryServer } from "mongodb-memory-server";
import { createSharedRateLimit } from "./sharedRateLimit.js";
import RateLimit from "../models/authRateLimit.model.js";

test("shared limits are atomic across middleware instances and expire by window", async () => {
  const database = await MongoMemoryServer.create();
  try {
    await mongoose.connect(database.getUri("shared-limiter-tests"));
    let time = 1700000000000;
    const options = { scope: "login", limit: 2, windowMs: 10000, now: () => time, secret: () => "test-secret" };
    const first = createSharedRateLimit(options), second = createSharedRateLimit(options);
    const request = (middleware, ip = "198.51.100.20") => new Promise(resolve => {
      let status = 200, headers = {};
      const res = { set: (name, value) => { headers[name] = value; }, status(code) { status = code; return this; }, json() { resolve({ status, headers }); } };
      middleware({ ip }, res, () => resolve({ status, headers }));
    });
    const results = await Promise.all([request(first), request(second), request(first), request(second)]);
    assert.deepEqual(results.map(x => x.status).sort(), [200, 200, 429, 429]);
    assert.ok(Number(results.find(x => x.status === 429).headers["Retry-After"]) > 0);
    assert.equal((await request(first, "198.51.100.21")).status, 200);
    const rows = await RateLimit.find().lean();
    assert.ok(rows.every(row => !JSON.stringify(row).includes("198.51.100")));
    const indexes = await RateLimit.collection.indexes();
    assert.ok(indexes.some(index => index.key.expiresAt === 1 && index.expireAfterSeconds === 0));
    time += 10000;
    assert.equal((await request(second)).status, 200);
    const missingSecret = createSharedRateLimit({ scope: "login", limit: 2, secret: () => "" });
    assert.equal((await request(missingSecret)).status, 503);
    const oldMode = process.env.MONGO_SHARED_AUTH_LIMITS, oldSecret = process.env.JWT_SECRET;
    process.env.MONGO_SHARED_AUTH_LIMITS = "true";
    process.env.JWT_SECRET = "shared-route-test-secret";
    const app = express();
    app.use(express.json());
    app.use(cookieParser());
    app.use("/auth", authRoutes);
    const server = await new Promise(resolve => { const instance = app.listen(0, "127.0.0.1", () => resolve(instance)); });
    try {
      const root = `http://127.0.0.1:${server.address().port}/auth`;
      const post = async path => (await fetch(root + path, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })).status;
      let status;
      for (let i = 0; i < 31; i++) status = await post("/login");
      assert.equal(status, 429);
      for (let i = 0; i < 11; i++) status = await post("/signup");
      assert.equal(status, 429);
      for (let i = 0; i < 31; i++) status = await post("/passkeys/login/verify");
      assert.equal(status, 429);
    } finally {
      await new Promise(resolve => server.close(resolve));
      if (oldMode === undefined) delete process.env.MONGO_SHARED_AUTH_LIMITS; else process.env.MONGO_SHARED_AUTH_LIMITS = oldMode;
      if (oldSecret === undefined) delete process.env.JWT_SECRET; else process.env.JWT_SECRET = oldSecret;
    }
  } finally {
    await mongoose.disconnect();
    await database.stop();
  }
});
