import test from "node:test";
import assert from "node:assert/strict";
import mongoose from "mongoose";
import { MongoMemoryServer } from "mongodb-memory-server";
import { ensureMessageRetentionIndexes } from "./retentionIndexes.js";
import Message from "../models/message.model.js";
import ChannelMessage from "../models/channelMessage.model.js";
import MatrixRoomEvent from "../models/matrixRoomEvent.model.js";
import MatrixToDevice from "../models/matrixToDevice.model.js";
import MatrixToDeviceTransaction from "../models/matrixToDeviceTransaction.model.js";
import MatrixEventTransaction from "../models/matrixEventTransaction.model.js";

test("retention indexes are created, verified, and conflicts fail without replacing them", async () => {
  const server = await MongoMemoryServer.create();
  try {
    await mongoose.connect(server.getUri("retention-indexes"), { autoIndex: false });
    await ensureMessageRetentionIndexes({ matrix: true });
    await ensureMessageRetentionIndexes({ matrix: true });
    for (const model of [Message, ChannelMessage, MatrixRoomEvent, MatrixToDevice, MatrixToDeviceTransaction, MatrixEventTransaction]) {
      assert.ok((await model.collection.indexes()).some(index => index.key.expiresAt === 1 && index.expireAfterSeconds === 0));
    }
    await Message.collection.dropIndex("expiresAt_1");
    await Message.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 3600 });
    await assert.rejects(ensureMessageRetentionIndexes(), /Message retention index unavailable/);
    assert.equal((await Message.collection.indexes()).find(index => index.key.expiresAt === 1).expireAfterSeconds, 3600);
    await Message.collection.dropIndex("expiresAt_1");
    await Message.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 0 });
    await MatrixToDevice.collection.dropIndex("expiresAt_1");
    await MatrixToDevice.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 3600 });
    await assert.rejects(ensureMessageRetentionIndexes({ matrix: true }), /Message retention index unavailable/);
    assert.equal((await MatrixToDevice.collection.indexes()).find(index => index.key.expiresAt === 1).expireAfterSeconds, 3600);
  } finally {
    await mongoose.disconnect();
    await server.stop();
  }
});
