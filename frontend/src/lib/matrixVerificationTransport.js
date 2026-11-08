// Experimental same-account SAS transport. Callers must hold the Matrix SDK
// store lock and handle user confirmation; server authorization is separate.
import { queueVerification, deliverQueuedVerification } from "./matrixVerificationOutbox.js";
import { authorizeVerifiedDevice } from "./matrixVerification.js";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const ID = /^[a-f0-9]{24}$/;
function validate(descriptor, api) {
  if (!descriptor || typeof descriptor.userId !== "string" || !USER.test(descriptor.userId) || typeof descriptor.deviceId !== "string" || !DEVICE.test(descriptor.deviceId) || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Invalid verification device.");
}

export async function sendMatrixVerification(sdk, machine, descriptor, targetDeviceId, request, api) {
  validate(descriptor, api);
  if (typeof targetDeviceId !== "string" || !DEVICE.test(targetDeviceId) || targetDeviceId === descriptor.deviceId) throw new Error("Invalid verification recipient.");
  await queueVerification(sdk, machine, descriptor.storeName, descriptor.userId, targetDeviceId, request);
  await deliverQueuedVerification(sdk, machine, descriptor.storeName, request.id, async prepared => {
    const result = (await api.post("/matrix/device/verification/send", { deviceId: descriptor.deviceId, transactionId: prepared.transactionId, eventType: prepared.eventType, messages: prepared.messages })).data;
    return result?.accepted === true;
  });
}

// The SDK persists its crypto state before the acknowledgement is sent. A
// repeated event after an ACK failure is fed to the SDK again; Matrix replay
// handling must remain enabled. A lost ACK does not imply device approval.
export async function receiveMatrixVerification(sdk, machine, descriptor, api) {
  validate(descriptor, api);
  if (!sdk?.DeviceLists || !machine || typeof machine.receiveSyncChangesMsc4186 !== "function") throw new Error("Verification is unavailable.");
  const rows = (await api.get("/matrix/device/verification/inbox", { params: { deviceId: descriptor.deviceId } })).data?.events;
  if (!Array.isArray(rows) || rows.length > 100 || rows.some(row => typeof row?.id !== "string" || !ID.test(row.id) || row.event?.type !== "m.room.encrypted" || row.event?.sender !== descriptor.userId || !row.event?.content || typeof row.event.content !== "object") || new Set(rows.map(row => row.id)).size !== rows.length) throw new Error("Invalid verification inbox.");
  if (!rows.length) return 0;
  const lists = new sdk.DeviceLists();
  try {
    const processed = await machine.receiveSyncChangesMsc4186(JSON.stringify(rows.map(row => row.event)), lists, new Map());
    for (const event of processed) event?.free?.();
  } finally { lists.free(); }
  const response = (await api.post("/matrix/device/verification/ack", { deviceId: descriptor.deviceId, ids: rows.map(row => row.id) })).data;
  if (response?.acknowledged !== true) throw new Error("Verification acknowledgement failed.");
  return rows.length;
}

// This only grants server-side access after the SDK says this exact SAS flow
// completed and the target key remains trusted locally.
export async function approveMatrixVerification(sdk, machine, descriptor, sas, targetDeviceId, api) {
  validate(descriptor, api);
  return authorizeVerifiedDevice(sdk, machine, sas, descriptor.userId, targetDeviceId, async target => {
    const result = (await api.post("/matrix/device/authorize", { deviceId: descriptor.deviceId, targetDeviceId: target.deviceId, expectedEd25519: target.ed25519 })).data;
    if (result?.authorized !== true || result.deviceId !== target.deviceId) throw new Error("Device approval was not confirmed.");
    return { userId: target.userId, deviceId: target.deviceId };
  });
}
