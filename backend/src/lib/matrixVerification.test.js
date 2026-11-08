// Synthetic same-account verification using the Matrix SDK, not production trust.
import { test } from "node:test";
import "fake-indexeddb/auto";
import assert from "node:assert/strict";
import mongoose from "mongoose";
import express from "express";
import cookieParser from "cookie-parser";
import jwt from "jsonwebtoken";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { OlmMachine, UserId, DeviceId, RequestType, DeviceLists, VerificationMethod } from "@matrix-org/matrix-sdk-crypto-wasm";
import MatrixDevice from "../models/matrixDevice.model.js";
import MatrixOneTimeKey from "../models/matrixOneTimeKey.model.js";
import MatrixToDevice from "../models/matrixToDevice.model.js";
import MatrixToDeviceTransaction from "../models/matrixToDeviceTransaction.model.js";
import MatrixBindingChallenge from "../models/matrixBindingChallenge.model.js";
import User from "../models/user.model.js";
import LoginSession from "../models/loginSession.model.js";
import { createDevice } from "./matrixKeys.js";
import { createMatrixDeviceRouter } from "../routes/matrixDevice.route.js";
import { createSession, sessionHash } from "./sessions.js";
import { issueMatrixBindingChallenge, bindMatrixDevice } from "./matrixSessionBinding.js";
import { queueVerification, deliverQueuedVerification } from "../../../frontend/src/lib/matrixVerificationOutbox.js";
import { authorizeVerifiedDevice } from "../../../frontend/src/lib/matrixVerification.js";
import { sendMatrixVerification, receiveMatrixVerification, approveMatrixVerification } from "../../../frontend/src/lib/matrixVerificationTransport.js";
import { publishMatrixVerificationKeys, refreshMatrixVerificationDevices, establishMatrixVerificationSession } from "../../../frontend/src/lib/matrixVerificationKeys.js";
import { incomingMatrixVerificationRequests, matrixVerificationFlow } from "../../../frontend/src/lib/matrixVerificationFlow.js";

// Test-only Web Locks implementation; real browsers supply this API.
const locks = new Map();
Object.defineProperty(globalThis.navigator, "locks", { configurable: true, value: { async request(name, _options, callback) {
  const previous = locks.get(name) || Promise.resolve();
  let release;
  const next = new Promise(resolve => { release = resolve; });
  locks.set(name, next);
  await previous;
  try { return await callback(); } finally { if (locks.get(name) === next) locks.delete(name); release(); }
} } });

test("two devices compare Matrix SAS codes over encrypted to-device events", async () => {
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
  const domain = "sonar.invalid";
  let first, second, server;
  try {
    await mongoose.connect(database.getUri("sonar-matrix-verification-tests"));
    await Promise.all([MatrixDevice.init(), MatrixOneTimeKey.init(), MatrixToDevice.init(), MatrixToDeviceTransaction.init(), MatrixBindingChallenge.init(), User.init(), LoginSession.init()]);
    const account = await User.create({ fullName: "Synthetic SAS", email: "sas@example.invalid", password: "unused-test-hash" });
    const accountId = String(account._id), user = `@${accountId}:${domain}`;
    process.env.JWT_SECRET = "synthetic-matrix-sas-http-test-secret";
    const raw = { FIRST: await createSession(accountId, 0, "password"), SECOND: await createSession(accountId, 0, "password") };
    const sessions = { FIRST: sessionHash(raw.FIRST), SECOND: sessionHash(raw.SECOND) };
    const cookie = id => `jwt=${jwt.sign({ userId: accountId, sessionVersion: 0, sessionId: raw[id] }, process.env.JWT_SECRET)}`;
    const app = express(); app.use(express.json()); app.use(cookieParser()); app.use("/device", createMatrixDeviceRouter(domain));
    await new Promise(resolve => { server = app.listen(0, "127.0.0.1", resolve); });
    const base = `http://127.0.0.1:${server.address().port}/device`;
    async function http(id, path, body) {
      const response = await fetch(base + path, { method: body ? "POST" : "GET", headers: { Cookie: cookie(id), ...(body ? { "Content-Type": "application/json" } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return response.json();
    }
    const apiFor = id => ({
      async get(path, options) { return { data: await http(id, path.replace("/matrix/device", "") + (options?.params ? `?${new URLSearchParams(options.params)}` : "")) }; },
      async post(path, body) { return { data: await http(id, path.replace("/matrix/device", ""), body) }; },
    });
    first = await OlmMachine.initialize(new UserId(user), new DeviceId("FIRST"));
    second = await OlmMachine.initialize(new UserId(user), new DeviceId("SECOND"));
    for (const [machine, id] of [[first, "FIRST"], [second, "SECOND"]]) {
      const upload = (await machine.outgoingRequests()).find(request => request.type === RequestType.KeysUpload);
      const body = JSON.parse(upload.body);
      await createDevice(user, id, body.device_keys);
      await MatrixDevice.updateOne({ userId: user, deviceId: id }, { $set: { serverAuthorization: id === "FIRST" ? "authorized" : "pending" } });
      const challenge = await issueMatrixBindingChallenge(accountId, sessions[id], user, id);
      const signatures = await machine.sign(challenge.message);
      const proof = JSON.parse(signatures.asJSON())[user][`ed25519:${id}`]; signatures.free();
      await bindMatrixDevice(accountId, sessions[id], user, id, challenge.token, proof);
      assert.equal(await publishMatrixVerificationKeys({ RequestType }, machine, { userId: user, deviceId: id }, apiFor(id)), true);
    }
    for (const [machine, id] of [[first, "FIRST"], [second, "SECOND"]]) {
      await refreshMatrixVerificationDevices({ UserId }, machine, { userId: user, deviceId: id }, apiFor(id));
    }
    const alteredDirectory = { ...apiFor("FIRST"), get: async () => ({ data: { device_keys: { [user]: {} }, failures: {}, master_keys: { unexpected: {} } } }) };
    await assert.rejects(refreshMatrixVerificationDevices({ UserId }, first, { userId: user, deviceId: "FIRST" }, alteredDirectory), /Invalid device key response/);
    await refreshMatrixVerificationDevices({ UserId }, first, { userId: user, deviceId: "FIRST" }, apiFor("FIRST"));
    assert.equal(await establishMatrixVerificationSession({ UserId, DeviceId }, first, { userId: user, deviceId: "FIRST" }, "SECOND", apiFor("FIRST")), true);
    await assert.rejects(http("SECOND", "/keys/claim-own", { deviceId: "FIRST", targetDeviceId: "SECOND", algorithm: "signed_curve25519" }), /HTTP 400/);
    let injectFailures = true;
    async function deliver(sender, recipient, senderId, recipientId, request) {
      assert.ok(request, "expected Matrix verification request");
      const sdk = { RequestType, UserId, DeviceId, DeviceLists };
      const storeName = `sonar-matrix-v2-${senderId === "FIRST" ? "a" : "b"}`.padEnd(48, senderId === "FIRST" ? "a" : "b");
      const prepared = await queueVerification(sdk, sender, storeName, user, recipientId, request);
      assert.deepEqual(await queueVerification(sdk, sender, storeName, user, recipientId, request), prepared);
      const changed = { id: request.id, txn_id: request.txn_id, type: request.type, event_type: request.event_type, body: `${request.body} ` };
      await assert.rejects(queueVerification(sdk, sender, storeName, user, recipientId, changed), /transaction changed/);
      await assert.rejects(deliverQueuedVerification(sdk, sender, storeName, request.id, async () => { throw new Error("offline"); }), /offline/);
      await assert.rejects(deliverQueuedVerification(sdk, sender, storeName, request.id, async () => false), /not accepted/);
      const send = value => http(senderId, "/verification/send", { deviceId: senderId, transactionId: value.transactionId, eventType: value.eventType, messages: value.messages });
      if (injectFailures) await assert.rejects(deliverQueuedVerification(sdk, sender, storeName, request.id, async value => { await send(value); throw new Error("lost response"); }), /lost response/);
      await sendMatrixVerification(sdk, sender, { userId: user, deviceId: senderId, storeName }, recipientId, request, apiFor(senderId));
      await assert.rejects(deliverQueuedVerification(sdk, sender, storeName, request.id, async () => true), /not found/);
      const receiverApi = apiFor(recipientId);
      const droppedAck = { ...receiverApi, post: async () => { throw new Error("ack offline"); } };
      const descriptor = { userId: user, deviceId: recipientId, storeName: `sonar-matrix-v2-${recipientId === "FIRST" ? "a" : "b"}`.padEnd(48, recipientId === "FIRST" ? "a" : "b") };
      if (injectFailures) await assert.rejects(receiveMatrixVerification(sdk, recipient, descriptor, droppedAck), /ack offline/);
      assert.equal(await receiveMatrixVerification(sdk, recipient, descriptor, receiverApi), 1);
      if (injectFailures) assert.equal(await receiveMatrixVerification(sdk, recipient, descriptor, receiverApi), 0);
      injectFailures = false;
    }
    const device = await first.getDevice(new UserId(user), new DeviceId("SECOND"));
    assert.equal(device.isVerified(), false);
    const [firstRequest, initial] = device.requestVerification([VerificationMethod.SasV1]);
    await deliver(first, second, "FIRST", "SECOND", initial);
    const secondRequest = second.getVerificationRequest(new UserId(user), firstRequest.flowId);
    assert.ok(secondRequest);
    await deliver(second, first, "SECOND", "FIRST", secondRequest.acceptWithMethods([VerificationMethod.SasV1]));
    const [firstSas, start] = await firstRequest.startSas();
    const firstDescriptor = { userId: user, deviceId: "FIRST", storeName: `sonar-matrix-v2-${"a".repeat(32)}` };
    const approve = value => http("FIRST", "/authorize", { deviceId: "FIRST", targetDeviceId: value.deviceId, expectedEd25519: value.ed25519 });
    await assert.rejects(approveMatrixVerification({ UserId, DeviceId }, first, firstDescriptor, firstSas, "SECOND", apiFor("FIRST")), /verification first/);
    assert.equal((await MatrixDevice.findOne({ userId: user, deviceId: "SECOND" })).serverAuthorization, "pending");
    await deliver(first, second, "FIRST", "SECOND", start);
    const secondSas = secondRequest.getVerification();
    assert.ok(secondSas);
    await deliver(second, first, "SECOND", "FIRST", secondSas.accept());
    for (const [sender, recipient, senderId, recipientId] of [[first, second, "FIRST", "SECOND"], [second, first, "SECOND", "FIRST"]]) {
      const pending = (await sender.outgoingRequests()).filter(request => request.type === RequestType.ToDevice && request.event_type === "m.key.verification.key");
      assert.equal(pending.length, 1);
      await deliver(sender, recipient, senderId, recipientId, pending[0]);
    }
    assert.deepEqual([...firstSas.emojiIndex()], [...secondSas.emojiIndex()]);
    assert.deepEqual([...firstSas.decimals()], [...secondSas.decimals()]);
    for (const request of await firstSas.confirm()) await deliver(first, second, "FIRST", "SECOND", request);
    for (const request of await secondSas.confirm()) await deliver(second, first, "SECOND", "FIRST", request);
    for (const [sender, recipient, senderId, recipientId] of [[first, second, "FIRST", "SECOND"], [second, first, "SECOND", "FIRST"]]) {
      for (const request of (await sender.outgoingRequests()).filter(item => item.type === RequestType.ToDevice && item.event_type === "m.key.verification.done")) await deliver(sender, recipient, senderId, recipientId, request);
    }
    assert.equal(firstSas.isDone(), true);
    assert.equal(secondSas.isDone(), true);
    assert.equal((await first.getDevice(new UserId(user), new DeviceId("SECOND"))).isVerified(), true);
    assert.equal((await second.getDevice(new UserId(user), new DeviceId("FIRST"))).isVerified(), true);
    await assert.rejects(http("SECOND", "/authorize", { deviceId: "SECOND", targetDeviceId: "FIRST", expectedEd25519: "A".repeat(43) }), /HTTP 400/);
    await assert.rejects(http("FIRST", "/authorize", { deviceId: "FIRST", targetDeviceId: "SECOND", expectedEd25519: "A".repeat(43) }), /HTTP 400/);
    await assert.rejects(authorizeVerifiedDevice({ UserId, DeviceId }, first, firstSas, user, "FIRST", approve), /verification first/);
    await assert.rejects(authorizeVerifiedDevice({ UserId, DeviceId }, second, firstSas, user, "SECOND", approve), /verification first/);
    const approval = await approveMatrixVerification({ UserId, DeviceId }, first, firstDescriptor, firstSas, "SECOND", apiFor("FIRST"));
    assert.deepEqual(approval, { userId: user, deviceId: "SECOND" });
    const approvedDevice = await MatrixDevice.findOne({ userId: user, deviceId: "SECOND" });
    assert.equal(approvedDevice.serverAuthorization, "authorized");
    assert.equal(approvedDevice.authorizedBy, "FIRST");
    await assert.rejects(approveMatrixVerification({ UserId, DeviceId }, first, firstDescriptor, firstSas, "SECOND", apiFor("FIRST")), /HTTP 400/);
    await LoginSession.deleteOne({ _id: sessions.SECOND });
    await assert.rejects(http("SECOND", "/keys/own?deviceId=SECOND"), /HTTP 401/);
    await assert.rejects(http("SECOND", "/keys/upload", { deviceId: "SECOND", oneTimeKeys: {} }), /HTTP 401/);
    await assert.rejects(http("SECOND", "/keys/claim-own", { deviceId: "SECOND", targetDeviceId: "FIRST", algorithm: "signed_curve25519" }), /HTTP 401/);
  } finally {
    first?.close(); second?.close();
    if (server) await new Promise(resolve => server.close(resolve));
    await mongoose.disconnect(); await database.stop();
  }
});

test("verification flow requires an explicit matching code before approval", async () => {
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
  let server, first, second;
  try {
    await mongoose.connect(database.getUri("sonar-matrix-flow-tests"));
    await Promise.all([MatrixDevice.init(), MatrixOneTimeKey.init(), MatrixToDevice.init(), MatrixToDeviceTransaction.init(), MatrixBindingChallenge.init(), User.init(), LoginSession.init()]);
    const account = await User.create({ fullName: "Synthetic flow", email: "flow@example.invalid", password: "unused-test-hash" });
    const accountId = String(account._id), user = `@${accountId}:sonar.invalid`;
    process.env.JWT_SECRET = "synthetic-matrix-flow-http-test-secret";
    const raw = { FIRST: await createSession(accountId, 0, "password"), SECOND: await createSession(accountId, 0, "password") };
    const sessions = { FIRST: sessionHash(raw.FIRST), SECOND: sessionHash(raw.SECOND) };
    const app = express(); app.use(express.json()); app.use(cookieParser()); app.use("/device", createMatrixDeviceRouter("sonar.invalid"));
    await new Promise(resolve => { server = app.listen(0, "127.0.0.1", resolve); });
    const base = `http://127.0.0.1:${server.address().port}/device`;
    const apiFor = id => {
      const cookie = `jwt=${jwt.sign({ userId: accountId, sessionVersion: 0, sessionId: raw[id] }, process.env.JWT_SECRET)}`;
      const call = async (path, options = {}) => {
        const response = await fetch(base + path, { ...options, headers: { Cookie: cookie, ...(options.body ? { "Content-Type": "application/json" } : {}) } });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return { data: await response.json() };
      };
      return { get: (path, options) => call(path.replace("/matrix/device", "") + (options?.params ? `?${new URLSearchParams(options.params)}` : "")), post: (path, body) => call(path.replace("/matrix/device", ""), { method: "POST", body: JSON.stringify(body) }) };
    };
    first = await OlmMachine.initialize(new UserId(user), new DeviceId("FIRST"));
    second = await OlmMachine.initialize(new UserId(user), new DeviceId("SECOND"));
    const sdk = { RequestType, UserId, DeviceId, DeviceLists, VerificationMethod };
    const desc = id => ({ userId: user, deviceId: id, storeName: `sonar-matrix-v2-${(id === "FIRST" ? "a" : "b").repeat(32)}` });
    for (const [machine, id] of [[first, "FIRST"], [second, "SECOND"]]) {
      const upload = (await machine.outgoingRequests()).find(item => item.type === RequestType.KeysUpload);
      await createDevice(user, id, JSON.parse(upload.body).device_keys);
      await MatrixDevice.updateOne({ userId: user, deviceId: id }, { $set: { serverAuthorization: id === "FIRST" ? "authorized" : "pending" } });
      const challenge = await issueMatrixBindingChallenge(accountId, sessions[id], user, id);
      const signatures = await machine.sign(challenge.message);
      const proof = JSON.parse(signatures.asJSON())[user][`ed25519:${id}`]; signatures.free();
      await bindMatrixDevice(accountId, sessions[id], user, id, challenge.token, proof);
      await publishMatrixVerificationKeys(sdk, machine, desc(id), apiFor(id));
    }
    await refreshMatrixVerificationDevices(sdk, first, desc("FIRST"), apiFor("FIRST"));
    await refreshMatrixVerificationDevices(sdk, second, desc("SECOND"), apiFor("SECOND"));
    await establishMatrixVerificationSession(sdk, first, desc("FIRST"), "SECOND", apiFor("FIRST"));
    const firstApi = apiFor("FIRST");
    let loseFirstResponse = true;
    const flakyApi = { ...firstApi, async post(path, body) {
      const result = await firstApi.post(path, body);
      if (path.endsWith("/verification/send") && loseFirstResponse) { loseFirstResponse = false; throw new Error("lost response"); }
      return result;
    } };
    const a = matrixVerificationFlow(sdk, first, desc("FIRST"), "SECOND", flakyApi);
    const b = matrixVerificationFlow(sdk, second, desc("SECOND"), "FIRST", apiFor("SECOND"));
    try {
      await assert.rejects(a.start(), /lost response/);
      assert.equal(a.hasPendingDelivery, true);
      const flow = a.id;
      assert.equal(await a.retryDelivery(), true);
      assert.equal(a.hasPendingDelivery, false);
      await b.receive();
      assert.deepEqual(incomingMatrixVerificationRequests(sdk, second, desc("SECOND")), [{ flowId: flow, deviceId: "FIRST" }]);
      assert.deepEqual(incomingMatrixVerificationRequests(sdk, first, desc("FIRST")), []);
      await b.accept(flow);
      await a.receive(); await a.startSas();
      await b.receive(); await b.acceptSas();
      await a.receive(); await a.flush();
      await b.receive(); await b.flush(); await a.receive();
      assert.deepEqual(a.codes(), b.codes());
      await assert.rejects(a.confirmCodesMatch(false), /Compare the codes/);
      await assert.rejects(a.approve(), /verification first/i);
      await a.confirmCodesMatch(true); await b.receive();
      await b.confirmCodesMatch(true); await a.receive();
      await a.flush(); await b.flush(); await a.receive(); await b.receive();
      assert.deepEqual(await a.approve(), { userId: user, deviceId: "SECOND" });
      assert.equal((await MatrixDevice.findOne({ userId: user, deviceId: "SECOND" })).serverAuthorization, "authorized");
    } finally { a.close(); b.close(); }
  } finally {
    first?.close(); second?.close();
    if (server) await new Promise(resolve => server.close(resolve));
    await mongoose.disconnect(); await database.stop();
  }
});
