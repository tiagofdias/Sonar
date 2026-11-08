// Stateful SAS interaction for one same-account device pair. The owner must keep
// the Matrix store locked and machine alive for the whole interaction. Closing
// a page may interrupt SAS; it does not approve or replace any identity.
import { sendMatrixVerification, receiveMatrixVerification, approveMatrixVerification } from "./matrixVerificationTransport.js";

const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const FLOW = /^[A-Za-z0-9._~-]{1,128}$/;
const EVENTS = new Set(["m.key.verification.request", "m.key.verification.ready", "m.key.verification.start", "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac", "m.key.verification.done", "m.key.verification.cancel"]);

// Called after receiving encrypted to-device events while holding the SDK
// store lock. Only live requests from another device of the same account are
// surfaced; the user still has to accept and compare the SAS on both devices.
export function incomingMatrixVerificationRequests(sdk, machine, descriptor) {
  if (!sdk?.UserId || !machine || typeof descriptor?.userId !== "string" || typeof descriptor.deviceId !== "string" || !DEVICE.test(descriptor.deviceId)) throw new Error("Invalid verification identity.");
  const user = new sdk.UserId(descriptor.userId);
  let requests;
  try { requests = machine.getVerificationRequests(user); }
  finally { user.free(); }
  if (!Array.isArray(requests) || requests.length > 100) { requests?.forEach(request => request.free()); throw new Error("Too many verification requests."); }
  const incoming = [];
  try {
    for (const request of requests) {
      const otherUser = request.otherUserId, otherDevice = request.otherDeviceId;
      try {
        const target = otherDevice?.toString(), flowId = request.flowId;
        if (!request.weStarted() && request.isSelfVerification() && !request.isCancelled() && !request.isDone() && !request.timedOut() && otherUser.toString() === descriptor.userId && typeof target === "string" && DEVICE.test(target) && target !== descriptor.deviceId && FLOW.test(flowId)) incoming.push({ flowId, deviceId: target });
      } finally { otherUser.free(); otherDevice?.free(); }
    }
  } finally { requests.forEach(request => request.free()); }
  return incoming;
}

export function matrixVerificationFlow(sdk, machine, descriptor, targetDeviceId, api) {
  if (!sdk?.VerificationMethod || !machine || !descriptor || typeof descriptor.userId !== "string" || typeof descriptor.deviceId !== "string" || typeof targetDeviceId !== "string" || !DEVICE.test(targetDeviceId) || targetDeviceId === descriptor.deviceId) throw new Error("Invalid verification flow.");
  let request, sas, flowId, pending;
  const readyToSend = () => { if (pending) throw new Error("Retry the pending verification event first."); };
  async function send(item) {
    if (!item) return;
    if (pending && pending.id !== item.id) throw new Error("Retry the pending verification event first.");
    pending = item;
    await sendMatrixVerification(sdk, machine, descriptor, targetDeviceId, item, api);
    pending = undefined;
  }
  function checkRequest(value) {
    if (!value || !value.isSelfVerification() || value.isCancelled() || value.timedOut() || value.flowId !== flowId) throw new Error("Verification is unavailable or expired.");
    const user = value.otherUserId, device = value.otherDeviceId;
    const completedDevice = !device && value.isDone() && sas ? sas.otherDeviceId : undefined;
    try { if (user.toString() !== descriptor.userId || (device || completedDevice)?.toString() !== targetDeviceId) throw new Error("Verification device changed."); }
    finally { user.free(); device?.free(); completedDevice?.free(); }
    return value;
  }
  function refreshSas() {
    const current = checkRequest(request).getVerification();
    if (current) { sas?.free(); sas = current; }
    if (!sas || typeof sas.decimals !== "function" || sas.isCancelled() || sas.timedOut()) throw new Error("Verification is unavailable or expired.");
    return sas;
  }
  return {
    get id() { return flowId; },
    get hasPendingDelivery() { return Boolean(pending); },
    async retryDelivery() { if (!pending) return false; await send(pending); return true; },
    async start() {
      if (request) throw new Error("Verification already started.");
      const user = new sdk.UserId(descriptor.userId), id = new sdk.DeviceId(targetDeviceId);
      let target;
      try {
        target = await machine.getDevice(user, id);
        if (!target || target.isDeleted() || target.isBlacklisted()) throw new Error("Verification device is unavailable.");
        const [created, initial] = target.requestVerification([sdk.VerificationMethod.SasV1]);
        request = created; flowId = request.flowId;
        if (!FLOW.test(flowId)) throw new Error("Invalid verification flow.");
        await send(initial);
        return flowId;
      } finally { target?.free(); user.free(); id.free(); }
    },
    async accept(id) {
      readyToSend();
      if (request || typeof id !== "string" || !FLOW.test(id)) throw new Error("Invalid verification flow.");
      flowId = id;
      const user = new sdk.UserId(descriptor.userId);
      try { request = machine.getVerificationRequest(user, id); }
      finally { user.free(); }
      await send(checkRequest(request).acceptWithMethods([sdk.VerificationMethod.SasV1]));
    },
    async receive() { return receiveMatrixVerification(sdk, machine, descriptor, api); },
    async startSas() {
      readyToSend();
      const started = await checkRequest(request).startSas();
      if (!started) throw new Error("Verification is not ready.");
      [sas] = started; await send(started[1]);
    },
    async acceptSas() { readyToSend(); await send(refreshSas().accept()); },
    async flush() {
      readyToSend();
      checkRequest(request);
      for (const item of await machine.outgoingRequests()) {
        if (item.type !== sdk.RequestType.ToDevice || !EVENTS.has(item.event_type)) continue;
        let body;
        try { body = JSON.parse(item.body); } catch { throw new Error("Invalid verification event."); }
        if (body?.messages?.[descriptor.userId]?.[targetDeviceId]?.transaction_id === flowId) await send(item);
      }
    },
    codes() {
      const current = refreshSas(), values = current.decimals();
      if (!current.canBePresented() || !values || values.length !== 3) throw new Error("Verification codes are not ready.");
      return Array.from(values);
    },
    async confirmCodesMatch(match) {
      readyToSend();
      if (match !== true) throw new Error("Compare the codes before confirming.");
      const current = refreshSas();
      if (!current.canBePresented()) throw new Error("Verification codes are not ready.");
      for (const item of await current.confirm()) await send(item);
    },
    async approve() { readyToSend(); return approveMatrixVerification(sdk, machine, descriptor, refreshSas(), targetDeviceId, api); },
    async cancel() { readyToSend(); await send(sas ? sas.cancel() : checkRequest(request).cancel()); },
    close() { sas?.free(); request?.free(); sas = undefined; request = undefined; pending = undefined; },
  };
}
