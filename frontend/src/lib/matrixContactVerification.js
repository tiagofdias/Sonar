// Experimental room-scoped SAS between different accounts. The caller holds
// the SDK store lock for the full interaction; server authorization alone is
// never treated as contact trust.
import * as sdk from "@matrix-org/matrix-sdk-crypto-wasm";
import { queueVerification, deliverQueuedVerification } from "./matrixVerificationOutbox.js";
import { publishMatrixVerificationKeys } from "./matrixVerificationKeys.js";
import { processVerificationInbox, clearVerificationReceipts, purgeExpiredVerificationReceipts, isInterruptedVerificationReceipt } from "./matrixVerificationInbox.js";
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/, DEVICE = /^[A-Za-z0-9]{1,32}$/, FLOW = /^[A-Za-z0-9._~-]{1,128}$/, ID = /^[a-f0-9]{24}$/;
const EVENTS = new Set(["m.key.verification.request", "m.key.verification.ready", "m.key.verification.start", "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac", "m.key.verification.done", "m.key.verification.cancel"]);

export async function prepareContactVerification(machine, descriptor, room, targetUser, targetDevice, api) {
  if (!machine || !USER.test(descriptor?.userId || "") || !DEVICE.test(descriptor?.deviceId || "") || !USER.test(targetUser || "") || targetUser === descriptor.userId || !DEVICE.test(targetDevice || "") || !room || room.kind !== "direct" || !Array.isArray(room.members) || room.members.length !== 2 || !room.members.includes(targetUser) || !room.members.includes(descriptor.userId) || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Invalid contact verification.");
  const snapshot = (await api.get("/matrix/device/room", { params: { roomId: room.roomId, deviceId: descriptor.deviceId } })).data;
  if (snapshot?.roomId !== room.roomId || snapshot.revision !== room.revision || snapshot.kind !== "direct" || !Array.isArray(snapshot.members) || snapshot.members.length !== 2 || !snapshot.members.includes(targetUser) || !snapshot.members.includes(descriptor.userId)) throw new Error("Room membership changed.");
  await publishMatrixVerificationKeys(sdk, machine, descriptor, api);
  await machine.updateTrackedUsers(room.members.map(user => new sdk.UserId(user)));
  const query = machine.queryKeysForUsers(room.members.map(user => new sdk.UserId(user)));
  const result = (await api.post("/matrix/messages/keys/query", { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId, users: room.members })).data;
  if (!result?.device_keys || Object.keys(result.device_keys).length !== 2 || !result.device_keys[descriptor.userId]?.[descriptor.deviceId] || !result.device_keys[targetUser]?.[targetDevice] || Object.keys(result.failures || {}).length !== 0 || result.device_keys[descriptor.userId][descriptor.deviceId]?.keys?.[`ed25519:${descriptor.deviceId}`] !== descriptor.ed25519) throw new Error("Contact keys unavailable.");
  await machine.markRequestAsSent(query.id, query.type, JSON.stringify(result));
  const user = new sdk.UserId(targetUser), deviceId = new sdk.DeviceId(targetDevice);
  let device, key, curve, targetCurve;
  try {
    device = await machine.getDevice(user, deviceId); key = device?.ed25519Key; curve = device?.curve25519Key;
    if (!device || device.isDeleted() || device.isBlacklisted() || !key || !curve || key.toBase64() !== result.device_keys[targetUser][targetDevice]?.keys?.[`ed25519:${targetDevice}`] || curve.toBase64() !== result.device_keys[targetUser][targetDevice]?.keys?.[`curve25519:${targetDevice}`]) throw new Error("Contact identity changed.");
    targetCurve = curve.toBase64();
  } finally { curve?.free(); key?.free(); device?.free(); user.free(); deviceId.free(); }
  const claim = await machine.getMissingSessions([new sdk.UserId(targetUser)]);
  if (claim) {
    let body; try { body = JSON.parse(claim.body); } catch { throw new Error("Invalid contact key claim."); }
    const algorithm = body?.one_time_keys?.[targetUser]?.[targetDevice];
    if (algorithm) {
      if (algorithm !== "signed_curve25519") throw new Error("Invalid contact key claim.");
      const response = (await api.post("/matrix/messages/keys/claim", { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId, targetUser, targetDevice, algorithm })).data;
      const keys = response?.one_time_keys?.[targetUser]?.[targetDevice];
      if (!keys || Object.keys(keys).length !== 1 || Object.keys(response.one_time_keys).length !== 1 || Object.keys(response.one_time_keys[targetUser]).length !== 1 || Object.keys(response.failures || {}).length !== 0) throw new Error("Contact one-time key unavailable.");
      await machine.markRequestAsSent(claim.id, claim.type, JSON.stringify(response));
    }
  }
  return contactFlow(machine, descriptor, room, targetUser, targetDevice, targetCurve, api);
}

function contactFlow(machine, descriptor, room, targetUser, targetDevice, targetCurve, api) {
  let request, sas, flowId, pending, abandoned = false;
  const ready = () => { if (abandoned) throw new Error("Start a new verification."); if (pending) throw new Error("Retry pending delivery first."); };
  async function send(item) {
    if (!item) return;
    if (pending && pending.id !== item.id) throw new Error("Retry pending delivery first.");
    pending = item;
    await queueVerification(sdk, machine, descriptor.storeName, targetUser, targetDevice, item);
    await deliverQueuedVerification(sdk, machine, descriptor.storeName, item.id, async prepared => {
      const result = (await api.post("/matrix/messages/verification/send", { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId, transactionId: prepared.transactionId, eventType: prepared.eventType, messages: prepared.messages })).data;
      return result?.accepted === true;
    });
    pending = undefined;
  }
  function check(value) {
    if (!value || value.isSelfVerification() || value.isCancelled() || value.timedOut() || value.flowId !== flowId) throw new Error("Contact verification unavailable.");
    const user = value.otherUserId, device = value.otherDeviceId, completed = !device && value.isDone() && sas ? sas.otherDeviceId : undefined;
    try { const selected = device || completed; if (user.toString() !== targetUser || (selected ? selected.toString() !== targetDevice : !(value.weStarted() && !value.isReady()))) throw new Error("Contact identity changed."); }
    finally { user.free(); device?.free(); completed?.free(); }
    return value;
  }
  function currentSas() {
    const current = check(request).getVerification();
    if (current) { sas?.free(); sas = current; }
    if (!sas || typeof sas.decimals !== "function" || sas.isCancelled() || sas.timedOut()) throw new Error("Contact verification unavailable.");
    return sas;
  }
  return {
    get id() { return flowId; },
    get hasPendingDelivery() { return Boolean(pending); },
    async retryDelivery() { if (!pending) return false; await send(pending); return true; },
    incoming() {
      const user = new sdk.UserId(targetUser); let requests;
      try { requests = machine.getVerificationRequests(user); } finally { user.free(); }
      if (!Array.isArray(requests) || requests.length > 100) { requests?.forEach(item => item.free()); throw new Error("Too many verification requests."); }
      const result = [];
      try { for (const item of requests) { const other = item.otherUserId, device = item.otherDeviceId; try { if (!item.weStarted() && !item.isSelfVerification() && !item.isDone() && !item.isCancelled() && !item.timedOut() && other.toString() === targetUser && device?.toString() === targetDevice && FLOW.test(item.flowId)) result.push({ flowId: item.flowId, userId: targetUser, deviceId: targetDevice }); } finally { other.free(); device?.free(); } } }
      finally { requests.forEach(item => item.free()); }
      return result;
    },
    async start() { if (request) throw new Error("Already started."); const user = new sdk.UserId(targetUser), id = new sdk.DeviceId(targetDevice); let device; try { device = await machine.getDevice(user, id); if (!device) throw new Error("Contact unavailable."); [request, pending] = device.requestVerification([sdk.VerificationMethod.SasV1]); flowId = request.flowId; if (!FLOW.test(flowId)) throw new Error("Invalid verification flow."); await send(pending); return flowId; } finally { device?.free(); user.free(); id.free(); } },
    async receive() {
      if (abandoned) throw new Error("Start a new verification.");
      const rows = (await api.get("/matrix/messages/verification/inbox", { params: { deviceId: descriptor.deviceId, roomId: room.roomId } })).data?.events;
      if (!Array.isArray(rows) || rows.length > 100 || rows.some(row => !ID.test(row?.id || "") || row.event?.type !== "m.room.encrypted" || row.event?.sender !== targetUser) || new Set(rows.map(row => row.id)).size !== rows.length) throw new Error("Invalid contact verification inbox.");
      await purgeExpiredVerificationReceipts();
      if (!rows.length) return 0;
      const ids = await processVerificationInbox(machine, descriptor, rows, sdk, async event => {
        const info = event.encryptionInfo, user = info?.sender, device = info?.senderDevice;
        try { if (!info || !user || !device || user.toString() !== targetUser || device.toString() !== targetDevice || info.senderCurve25519Key !== targetCurve) throw new Error("Contact identity changed."); }
        finally { device?.free(); user?.free(); info?.free(); }
      });
      const result = (await api.post("/matrix/messages/verification/ack", { roomId: room.roomId, deviceId: descriptor.deviceId, ids })).data;
      if (result?.acknowledged !== true) throw new Error("Verification acknowledgement failed.");
      await clearVerificationReceipts(descriptor, ids); return ids.length;
    },
    // Explicitly abandon the ambiguous event. The caller must then close this
    // flow and start another; this action never confirms or verifies a device.
    async abandonInterrupted(eventId) {
      if (!ID.test(eventId || "") || abandoned) throw new Error("Invalid interrupted event.");
      const rows = (await api.get("/matrix/messages/verification/inbox", { params: { deviceId: descriptor.deviceId, roomId: room.roomId } })).data?.events;
      if (!Array.isArray(rows)) throw new Error("Invalid contact verification inbox.");
      const row = rows.find(value => value?.id === eventId && value.event?.sender === targetUser && value.event?.type === "m.room.encrypted");
      if (!row || !await isInterruptedVerificationReceipt(descriptor, row)) throw new Error("Interrupted event unavailable.");
      abandoned = true;
      const result = (await api.post("/matrix/messages/verification/ack", { roomId: room.roomId, deviceId: descriptor.deviceId, ids: [eventId] })).data;
      if (result?.acknowledged !== true) throw new Error("Could not discard interrupted event.");
      await clearVerificationReceipts(descriptor, [eventId]);
      return true;
    },
    async accept(id) { ready(); if (request || !FLOW.test(id || "")) throw new Error("Invalid verification flow."); flowId = id; const user = new sdk.UserId(targetUser); try { request = machine.getVerificationRequest(user, id); } finally { user.free(); } await send(check(request).acceptWithMethods([sdk.VerificationMethod.SasV1])); },
    async startSas() { ready(); const started = await check(request).startSas(); if (!started) throw new Error("Verification not ready."); [sas] = started; await send(started[1]); },
    async acceptSas() { ready(); await send(currentSas().accept()); },
    async flush() { ready(); check(request); for (const item of await machine.outgoingRequests()) { if (item.type !== sdk.RequestType.ToDevice || !EVENTS.has(item.event_type)) continue; const body = JSON.parse(item.body); if (body?.messages?.[targetUser]?.[targetDevice]?.transaction_id === flowId) await send(item); } },
    codes() { const current = currentSas(), values = current.decimals(); if (!current.canBePresented() || !values || values.length !== 3) throw new Error("Codes not ready."); return Array.from(values); },
    async confirm(match) { ready(); if (match !== true) throw new Error("Compare the codes first."); for (const item of await currentSas().confirm()) await send(item); },
    async cancel() { ready(); await send(sas ? sas.cancel() : check(request).cancel()); },
    verified() { if (abandoned) return false; const current = currentSas(); return current.isDone() && current.haveWeConfirmed() && !current.isCancelled() && !current.timedOut(); },
    close() { request?.free(); sas?.free(); request = undefined; sas = undefined; pending = undefined; },
  };
}
