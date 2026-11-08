import { test } from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import mongoose from "mongoose";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { OlmMachine, UserId, DeviceId, RequestType } from "@matrix-org/matrix-sdk-crypto-wasm";
import User from "../models/user.model.js";
import LoginSession from "../models/loginSession.model.js";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixBindingChallenge from "../models/matrixBindingChallenge.model.js";
import { createDevice, revokeDevice } from "./matrixKeys.js";
import { issueMatrixBindingChallenge, bindMatrixDevice, boundMatrixDevice, activeMatrixLogin } from "./matrixSessionBinding.js";
import { signMatrixBinding } from "../../../frontend/src/lib/matrixBinding.js";

test("session binding requires a fresh signature from the account's Matrix device", async () => {
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
  let first, other;
  try {
    await mongoose.connect(database.getUri("sonar-matrix-session-tests"));
    await Promise.all([User.init(), LoginSession.init(), MatrixDevice.init(), MatrixBindingChallenge.init()]);
    const account = await User.create({ fullName: "Synthetic user", email: "binding@example.invalid", password: "unused-test-hash" });
    const accountId = String(account._id), userId = `@${accountId}:sonar.invalid`;
    const sessionId = randomBytes(32).toString("hex"), otherSessionId = randomBytes(32).toString("hex");
    for (const id of [sessionId, otherSessionId]) await LoginSession.create({ _id: id, userId: account._id, sessionVersion: 0, method: "password", expiresAt: new Date(Date.now() + 60000) });
    first = await OlmMachine.initialize(new UserId(userId), new DeviceId("FIRST"));
    other = await OlmMachine.initialize(new UserId(userId), new DeviceId("OTHER"));
    for (const [machine, id] of [[first, "FIRST"], [other, "OTHER"]]) {
      const upload = (await machine.outgoingRequests()).find(request => request.type === RequestType.KeysUpload);
      await createDevice(userId, id, JSON.parse(upload.body).device_keys);
    }
    async function sign(machine, device, message) {
      const signatures = await machine.sign(message);
      try { return JSON.parse(signatures.asJSON())[userId][`ed25519:${device}`]; }
      finally { signatures.free(); }
    }
    await assert.rejects(boundMatrixDevice(accountId, sessionId), /Session unavailable/);
    const challenge = await issueMatrixBindingChallenge(accountId, sessionId, userId, "FIRST");
    const proof = await signMatrixBinding(first, userId, "FIRST", { ...challenge, expiresAt: challenge.expiresAt.toISOString() });
    await assert.rejects(signMatrixBinding(first, userId, "FIRST", { ...challenge, expiresAt: challenge.expiresAt.toISOString(), message: `${challenge.message}\nother` }), /Invalid device binding/);
    await assert.rejects(signMatrixBinding(other, userId, "FIRST", { ...challenge, expiresAt: challenge.expiresAt.toISOString() }), /does not match/);
    await assert.rejects(bindMatrixDevice(accountId, otherSessionId, userId, "FIRST", challenge.token, proof), /unavailable/);
    await assert.rejects(bindMatrixDevice(accountId, sessionId, userId, "FIRST", challenge.token, await sign(other, "OTHER", challenge.message)), /Invalid device signature/);
    const attempts = await Promise.allSettled(Array.from({ length: 3 }, () => bindMatrixDevice(accountId, sessionId, userId, "FIRST", challenge.token, proof)));
    assert.equal(attempts.filter(item => item.status === "fulfilled").length, 1);
    assert.deepEqual(await boundMatrixDevice(accountId, sessionId), { userId, deviceId: "FIRST" });
    await assert.rejects(bindMatrixDevice(accountId, sessionId, userId, "FIRST", challenge.token, proof), /unavailable/);
    await assert.rejects(issueMatrixBindingChallenge(accountId, sessionId, userId, "OTHER"), /already bound/);
    const locked = await issueMatrixBindingChallenge(accountId, otherSessionId, userId, "FIRST");
    for (let index = 0; index < 5; index++) await assert.rejects(bindMatrixDevice(accountId, otherSessionId, userId, "FIRST", locked.token, "wrong"), /Invalid device signature/);
    await assert.rejects(bindMatrixDevice(accountId, otherSessionId, userId, "FIRST", locked.token, await sign(first, "FIRST", locked.message)), /unavailable/);
    const expiring = await issueMatrixBindingChallenge(accountId, otherSessionId, userId, "FIRST");
    await MatrixBindingChallenge.updateOne({ sessionId: otherSessionId, consumedAt: null, failures: 0 }, { $set: { expiresAt: new Date(0) } });
    await assert.rejects(bindMatrixDevice(accountId, otherSessionId, userId, "FIRST", expiring.token, await sign(first, "FIRST", expiring.message)), /unavailable/);
    await assert.rejects(issueMatrixBindingChallenge("000000000000000000000000", sessionId, userId, "FIRST"), /Invalid binding/);
    // A transaction that has already read a snapshot cannot authorize after a
    // concurrent sign-out: its write conflicts, then the retry sees revocation.
    let changedVersion = false;
    await assert.rejects(mongoose.connection.transaction(async session => {
      await User.findById(account._id).session(session);
      if (!changedVersion) {
        changedVersion = true;
        await User.updateOne({ _id: account._id }, { $inc: { sessionVersion: 1 } });
      }
      await activeMatrixLogin(session, accountId, sessionId);
    }), /Session unavailable/);
    await assert.rejects(boundMatrixDevice(accountId, sessionId), /Session unavailable/);
    await assert.rejects(issueMatrixBindingChallenge(accountId, otherSessionId, userId, "OTHER"), /Session unavailable/);
    await User.updateOne({ _id: account._id }, { $set: { sessionVersion: 0 } });
    assert.equal(await revokeDevice(userId, "FIRST"), true);
    await assert.rejects(boundMatrixDevice(accountId, sessionId), /Session unavailable/);
    assert.equal(await MatrixBindingChallenge.countDocuments({ userId, deviceId: "FIRST" }), 0);
  } finally {
    first?.close(); other?.close();
    await mongoose.disconnect(); await database.stop();
  }
});
