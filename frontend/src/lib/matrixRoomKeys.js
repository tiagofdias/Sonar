// Room-device lookup for the experimental Matrix transport. The caller holds
// the SDK store lock and supplies a freshly checked room revision. A server
// access flag is never treated as evidence of cryptographic trust.
import { DeviceId, UserId } from "@matrix-org/matrix-sdk-crypto-wasm";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
export async function requireTrustedRoomDevices(machine, descriptor, room, api) {
  if (!machine || !USER.test(descriptor?.userId || "") || !DEVICE.test(descriptor?.deviceId || "") || !room || typeof room.roomId !== "string" || !/^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/.test(room.roomId) || !Number.isSafeInteger(room.revision) || room.revision < 0 || !Array.isArray(room.members) || room.members.length < 2 || room.members.length > 32 || new Set(room.members).size !== room.members.length || !room.members.includes(descriptor.userId) || room.members.some(user => !USER.test(user)) || typeof api?.post !== "function") throw new Error("Invalid room identity.");
  {
    // The SDK consumes UserId objects passed in these arrays.
    await machine.updateTrackedUsers(room.members.map(user => new UserId(user)));
    const query = machine.queryKeysForUsers(room.members.map(user => new UserId(user)));
    const result = (await api.post("/matrix/messages/keys/query", { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId, users: room.members })).data;
    if (!result || !result.device_keys || typeof result.device_keys !== "object" || Array.isArray(result.device_keys) || !result.failures || typeof result.failures !== "object" || Array.isArray(result.failures) || Object.keys(result.failures).length || Object.keys(result.device_keys).length !== room.members.length || Object.keys(result.device_keys).some(user => !room.members.includes(user))) throw new Error("Invalid room device keys.");
    for (const user of room.members) {
      const devices = result.device_keys[user];
      if (!devices || typeof devices !== "object" || Array.isArray(devices) || Object.keys(devices).length < 1 || Object.keys(devices).length > 100 || Object.keys(devices).some(deviceId => !DEVICE.test(deviceId))) throw new Error("A room member has no usable device.");
    }
    await machine.markRequestAsSent(query.id, query.type, JSON.stringify(result));
    const trusted = [];
    for (const user of room.members) {
      for (const deviceId of Object.keys(result.device_keys[user])) {
        const userId = new UserId(user), id = new DeviceId(deviceId);
        let device, key, curve;
        try {
          device = await machine.getDevice(userId, id);
          key = device?.ed25519Key;
          curve = device?.curve25519Key;
          const expected = result.device_keys[user][deviceId]?.keys?.[`ed25519:${deviceId}`];
          const expectedCurve = result.device_keys[user][deviceId]?.keys?.[`curve25519:${deviceId}`];
          const local = user === descriptor.userId && deviceId === descriptor.deviceId;
          if (!device || device.isDeleted() || device.isBlacklisted() || !key || key.toBase64() !== expected || !curve || curve.toBase64() !== expectedCurve || (local && expected !== descriptor.ed25519) || (!local && !device.isVerified())) throw new Error("A room device is not verified. Sending is blocked.");
          trusted.push({ userId: user, deviceId, ed25519: expected, curve25519: expectedCurve });
        } finally { curve?.free(); key?.free(); device?.free(); userId.free(); id.free(); }
      }
    }
    return trusted;
  }
}

// Establish Olm sessions only after every listed device passed the SDK trust
// check above. A consumed one-time key whose response is lost may be wasted;
// no room key is shared until the SDK accepts a complete claim response.
export async function establishTrustedRoomSessions(machine, descriptor, room, api) {
  const trusted = await requireTrustedRoomDevices(machine, descriptor, room, api);
  const claim = await machine.getMissingSessions(room.members.map(user => new UserId(user)));
  if (!claim) return { trusted, claimed: 0 };
  let requests;
  try { requests = JSON.parse(claim.body)?.one_time_keys; } catch { throw new Error("Invalid key claim request."); }
  if (!requests || typeof requests !== "object" || Array.isArray(requests)) throw new Error("Invalid key claim request.");
  const approved = new Set(trusted.map(device => `${device.userId}\n${device.deviceId}`));
  const collected = {}, targets = [];
  for (const [userId, devices] of Object.entries(requests)) {
    if (!room.members.includes(userId) || !devices || typeof devices !== "object" || Array.isArray(devices)) throw new Error("Invalid key claim recipient.");
    for (const [deviceId, algorithm] of Object.entries(devices)) {
      if (!approved.has(`${userId}\n${deviceId}`) || (userId === descriptor.userId && deviceId === descriptor.deviceId) || algorithm !== "signed_curve25519") throw new Error("Unverified key claim recipient.");
      targets.push({ userId, deviceId, algorithm });
    }
  }
  for (const target of targets) {
    const result = (await api.post("/matrix/messages/keys/claim", { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId, targetUser: target.userId, targetDevice: target.deviceId, algorithm: target.algorithm })).data;
    const all = result?.one_time_keys, users = all && Object.keys(all), devices = all?.[target.userId], keys = devices?.[target.deviceId];
    if (!all || typeof all !== "object" || Array.isArray(all) || users.length !== 1 || users[0] !== target.userId || !devices || typeof devices !== "object" || Array.isArray(devices) || Object.keys(devices).length !== 1 || !keys || typeof keys !== "object" || Array.isArray(keys) || Object.keys(keys).length !== 1 || !/^signed_curve25519:[A-Za-z0-9+/=_-]{1,80}$/.test(Object.keys(keys)[0]) || !result.failures || typeof result.failures !== "object" || Array.isArray(result.failures) || Object.keys(result.failures).length) throw new Error("A trusted device has no available one-time key.");
    (collected[target.userId] ||= {})[target.deviceId] = keys;
  }
  await machine.markRequestAsSent(claim.id, claim.type, JSON.stringify({ one_time_keys: collected, failures: {} }));
  return { trusted, claimed: targets.length };
}
