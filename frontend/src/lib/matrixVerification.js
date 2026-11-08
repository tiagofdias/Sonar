// Matrix SAS messages are encrypted with the SDK's existing Olm session.
// This module is not yet connected to the UI or the server. Prepared delivery
// MUST be persisted before transmission and reused verbatim on every retry:
// encrypting again advances the ratchet and changes the ciphertext.
const EVENTS = new Set([
  "m.key.verification.request", "m.key.verification.ready", "m.key.verification.start",
  "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac",
  "m.key.verification.done", "m.key.verification.cancel",
]);
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const TRANSACTION = /^[A-Za-z0-9._~-]{1,128}$/;
const object = value => value !== null && typeof value === "object" && !Array.isArray(value);

export async function prepareVerificationDelivery(sdk, machine, userId, deviceId, request) {
  const { RequestType, UserId, DeviceId } = sdk;
  if (!machine || typeof userId !== "string" || typeof deviceId !== "string" || !DEVICE.test(deviceId) || request?.type !== RequestType.ToDevice || !EVENTS.has(request.event_type) || typeof request.id !== "string" || !TRANSACTION.test(request.id) || request.id !== request.txn_id) throw new Error("Invalid verification request.");
  let body;
  try { body = JSON.parse(request.body); } catch { throw new Error("Invalid verification request."); }
  const messages = body?.messages;
  if (!object(messages) || Object.keys(messages).length !== 1 || !object(messages[userId]) || Object.keys(messages[userId]).length !== 1 || !object(messages[userId][deviceId])) throw new Error("Verification requires exactly one device.");
  const target = await machine.getDevice(new UserId(userId), new DeviceId(deviceId));
  if (!target) throw new Error("The device is unavailable.");
  try {
    if (target.isDeleted() || target.isBlacklisted()) throw new Error("The device is unavailable.");
    const encrypted = JSON.parse(await target.encryptToDeviceEvent(request.event_type, messages[userId][deviceId]));
    return { requestId: request.id, transactionId: request.txn_id, eventType: "m.room.encrypted", messages: { [userId]: { [deviceId]: encrypted } } };
  } finally { target.free(); }
}

// Acknowledge the SDK only after the server has durably accepted the exact
// prepared ciphertext. A failed send must be retried with that same object.
export async function sendPreparedVerification(sdk, machine, prepared, send) {
  if (!machine || !prepared || typeof send !== "function" || typeof prepared.requestId !== "string" || !TRANSACTION.test(prepared.requestId) || prepared.requestId !== prepared.transactionId || prepared.eventType !== "m.room.encrypted" || !object(prepared.messages)) throw new Error("Invalid prepared verification.");
  if (await send(prepared) !== true) throw new Error("Verification delivery was not accepted.");
  await machine.markRequestAsSent(prepared.requestId, sdk.RequestType.ToDevice, "{}");
}

// Gate a future server-side authorization on the completed SAS flow and the
// currently trusted SDK device. Server authorization is only access control;
// it does not replace this check and cannot prove that users compared codes.
export async function authorizeVerifiedDevice(sdk, machine, sas, userId, deviceId, authorize) {
  if (!machine || !sas || typeof authorize !== "function" || typeof userId !== "string" || typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid verification approval.");
  const otherUser = sas.otherUserId, otherDevice = sas.otherDeviceId, localUser = sas.userId, localDevice = sas.deviceId, machineUser = machine.userId, machineDevice = machine.deviceId;
  try {
    if (!sas.isSelfVerification() || !sas.isDone() || !sas.haveWeConfirmed() || sas.isCancelled() || sas.timedOut() || otherUser.toString() !== userId || otherDevice.toString() !== deviceId || localUser.toString() !== machineUser.toString() || localDevice.toString() !== machineDevice.toString()) throw new Error("Complete the device verification first.");
  } finally { otherUser.free(); otherDevice.free(); localUser.free(); localDevice.free(); machineUser.free(); machineDevice.free(); }
  const targetUser = new sdk.UserId(userId), targetId = new sdk.DeviceId(deviceId);
  let target, key;
  try {
    target = await machine.getDevice(targetUser, targetId);
    if (!target || target.isDeleted() || target.isBlacklisted() || !target.isVerified()) throw new Error("The device is not trusted.");
    key = target.ed25519Key;
    if (!key) throw new Error("The device has no signing key.");
    return await authorize({ userId, deviceId, ed25519: key.toBase64() });
  } finally { key?.free(); target?.free(); targetUser.free(); targetId.free(); }
}
