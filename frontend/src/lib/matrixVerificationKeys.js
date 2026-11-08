// Experimental same-account SAS key setup. The caller holds the SDK store lock.
// These operations establish Olm sessions; they do not verify a device.
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
function validate(machine, descriptor, api) {
  if (!machine || typeof descriptor?.userId !== "string" || !USER.test(descriptor.userId) || typeof descriptor.deviceId !== "string" || !DEVICE.test(descriptor.deviceId) || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Invalid verification identity.");
  const user = machine.userId, device = machine.deviceId;
  try { if (user.toString() !== descriptor.userId || device.toString() !== descriptor.deviceId) throw new Error("Local verification identity changed."); }
  finally { user.free(); device.free(); }
}

export async function publishMatrixVerificationKeys(sdk, machine, descriptor, api) {
  validate(machine, descriptor, api);
  const request = (await machine.outgoingRequests()).find(item => item.type === sdk.RequestType.KeysUpload);
  if (!request) return false;
  let body;
  try { body = JSON.parse(request.body); } catch { throw new Error("Invalid local key upload."); }
  if (!body.one_time_keys || typeof body.one_time_keys !== "object" || Array.isArray(body.one_time_keys)) throw new Error("Invalid local key upload.");
  const result = (await api.post("/matrix/device/keys/upload", { deviceId: descriptor.deviceId, oneTimeKeys: body.one_time_keys })).data;
  if (!Number.isSafeInteger(result?.one_time_key_counts?.signed_curve25519) || result.one_time_key_counts.signed_curve25519 < 0) throw new Error("Invalid key upload response.");
  await machine.markRequestAsSent(request.id, request.type, JSON.stringify(result));
  return true;
}

export async function refreshMatrixVerificationDevices(sdk, machine, descriptor, api) {
  validate(machine, descriptor, api);
  await machine.updateTrackedUsers([new sdk.UserId(descriptor.userId)]);
  const query = machine.queryKeysForUsers([new sdk.UserId(descriptor.userId)]);
  const result = (await api.get("/matrix/device/keys/own", { params: { deviceId: descriptor.deviceId } })).data;
  if (!result || Object.keys(result).some(key => !["device_keys", "failures"].includes(key)) || !result.device_keys || typeof result.device_keys !== "object" || Array.isArray(result.device_keys) || Object.keys(result.device_keys).length !== 1 || !result.device_keys[descriptor.userId] || typeof result.device_keys[descriptor.userId] !== "object" || Array.isArray(result.device_keys[descriptor.userId]) || !result.failures || typeof result.failures !== "object" || Array.isArray(result.failures) || Object.keys(result.failures).length !== 0) throw new Error("Invalid device key response.");
  await machine.markRequestAsSent(query.id, query.type, JSON.stringify(result));
}

export async function establishMatrixVerificationSession(sdk, machine, descriptor, targetDeviceId, api) {
  validate(machine, descriptor, api);
  if (typeof targetDeviceId !== "string" || !DEVICE.test(targetDeviceId) || targetDeviceId === descriptor.deviceId) throw new Error("Invalid verification recipient.");
  const targetUser = new sdk.UserId(descriptor.userId), targetId = new sdk.DeviceId(targetDeviceId);
  let target;
  try {
    target = await machine.getDevice(targetUser, targetId);
    if (!target || target.isDeleted() || target.isBlacklisted()) throw new Error("Verification device is unavailable.");
  } finally { target?.free(); targetUser.free(); targetId.free(); }
  const claim = await machine.getMissingSessions([new sdk.UserId(descriptor.userId)]);
  if (!claim) return false;
  let payload;
  try { payload = JSON.parse(claim.body); } catch { throw new Error("Invalid local key claim."); }
  const requested = payload?.one_time_keys?.[descriptor.userId]?.[targetDeviceId];
  if (!requested) return false;
  if (requested !== "signed_curve25519") throw new Error("Unsupported verification key.");
  const result = (await api.post("/matrix/device/keys/claim-own", { deviceId: descriptor.deviceId, targetDeviceId, algorithm: requested })).data;
  const keyset = result?.one_time_keys?.[descriptor.userId]?.[targetDeviceId];
  if (!keyset || typeof keyset !== "object" || Array.isArray(keyset) || Object.keys(keyset).length !== 1 || !result.failures || typeof result.failures !== "object") throw new Error("Verification key unavailable.");
  await machine.markRequestAsSent(claim.id, claim.type, JSON.stringify(result));
  return true;
}
