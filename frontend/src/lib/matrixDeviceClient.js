// Experimental registration for an already-created local identity. The server
// only puts the device in pending state; this neither approves it nor migrates
// the legacy browser key. Never create a replacement identity on an error.
import { RequestType, RoomId } from "@matrix-org/matrix-sdk-crypto-wasm";
import { browserMatrixIdentityExists, createBrowserMatrixIdentity, withBrowserMatrixIdentity } from "./matrixIdentityVault.js";
import { signMatrixEnrollment } from "./matrixEnrollment.js";
import { signMatrixBinding } from "./matrixBinding.js";
import { ensureMatrixRoomRevision } from "./matrixRoomRevision.js";
import { proveExistingLegacyIdentity } from "./crypto.js";

// Called only from an explicit setup action. A shared lock prevents two tabs
// from creating competing devices; a retry always uses the existing identity.
export async function prepareBrowserMatrixDevice(accountId, api) {
  if (typeof accountId !== "string" || !/^[a-f0-9]{24}$/.test(accountId) || !api || typeof api.get !== "function" || typeof api.post !== "function" || !globalThis.navigator?.locks) throw new Error("Device service unavailable.");
  const userId = (await api.get("/matrix/device/identity")).data?.userId;
  if (typeof userId !== "string" || !new RegExp(`^@${accountId}:[a-z0-9.-]{1,200}$`).test(userId)) throw new Error("The server identity changed. Device setup is blocked.");
  return navigator.locks.request(`matrix-setup:${userId}`, { mode: "exclusive" }, async () => {
    if (!await browserMatrixIdentityExists(userId)) {
      const random = crypto.getRandomValues(new Uint8Array(12));
      const deviceId = Array.from(random, byte => byte.toString(16).padStart(2, "0")).join("").toUpperCase();
      await createBrowserMatrixIdentity(userId, deviceId);
    }
    return registerExistingBrowserMatrixDevice(userId, api);
  });
}

// Invoke only after an explicit first-device setup action. This authorizes
// access on the server; it is not SAS verification or history migration.
export async function bootstrapExistingBrowserMatrixIdentity(userId, api) {
  const accountId = /^@([a-f0-9]{24}):[a-z0-9.-]{1,200}$/.exec(userId)?.[1];
  if (!accountId || !api || typeof api.get !== "function" || typeof api.post !== "function") throw new Error("Device service unavailable.");
  if ((await api.get("/matrix/device/identity")).data?.userId !== userId) throw new Error("The server identity changed. Device setup is blocked.");
  return withBrowserMatrixIdentity(userId, async (_machine, { deviceId, ed25519 }) => {
    try {
      const challenge = (await api.post("/matrix/device/bootstrap/legacy/challenge", { deviceId })).data;
      const proof = await proveExistingLegacyIdentity(accountId, userId, deviceId, challenge);
      const result = (await api.post("/matrix/device/bootstrap/legacy/complete", { deviceId, token: challenge.token, proof })).data;
      if (result?.authorized !== true || result?.deviceId !== deviceId) throw new Error("Unexpected bootstrap response.");
    } catch (error) {
      // A committed response may be lost. Confirm the bound device and its
      // local signing key before treating a retry as successful; never create
      // a replacement or interpret server authorization as SAS verification.
      try {
        const devices = (await api.get("/matrix/device/list")).data?.devices;
        if (!Array.isArray(devices) || devices.length > 100) throw error;
        const current = devices.filter(device => device?.current === true);
        if (current.length !== 1 || current[0].deviceId !== deviceId || current[0].ed25519 !== ed25519 || current[0].serverAuthorization !== "authorized") throw error;
      } catch { throw error; }
    }
    return { userId, deviceId, status: "authorized" };
  });
}

export async function registerExistingBrowserMatrixDevice(userId, api) {
  if (!api || typeof api.get !== "function" || typeof api.post !== "function") throw new Error("Device service unavailable.");
  const identity = (await api.get("/matrix/device/identity")).data;
  if (identity?.userId !== userId) throw new Error("The server identity changed. Device setup is blocked.");
  return withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
    const { deviceId } = descriptor;
    const upload = (await machine.outgoingRequests()).find(request => request.type === RequestType.KeysUpload);
    let keys;
    try { keys = JSON.parse(upload?.body || "{}").device_keys; } catch { throw new Error("Local device keys are unavailable."); }
    if (!keys) throw new Error("Local device keys are unavailable.");
    // If completion succeeds but its response is lost, a fresh binding proof
    // recovers the operation without re-registering or replacing the key.
    let enrollmentError;
    try {
      const challenge = (await api.post("/matrix/device/enroll/challenge", { deviceId, deviceKeys: keys })).data;
      const signature = await signMatrixEnrollment(machine, userId, deviceId, keys, challenge);
      const result = (await api.post("/matrix/device/enroll/complete", { deviceId, token: challenge.token, signature })).data;
      if (result?.userId !== userId || result?.deviceId !== deviceId || result?.serverAuthorization !== "pending") throw new Error("Unexpected enrollment response.");
    } catch (error) { enrollmentError = error; }
    try {
      const challenge = (await api.post("/matrix/device/bind/challenge", { deviceId })).data;
      const signature = await signMatrixBinding(machine, userId, deviceId, challenge);
      const result = (await api.post("/matrix/device/bind/complete", { deviceId, token: challenge.token, signature })).data;
      if (result?.userId !== userId || result?.deviceId !== deviceId) throw new Error("Unexpected binding response.");
    } catch (error) {
      if (enrollmentError) throw new Error("Device registration could not be confirmed. The existing local identity was kept.", { cause: enrollmentError });
      throw error;
    }
    return { userId, deviceId, status: "verification-required" };
  });
}

// A server access flag is not a cryptographic trust state. The current public
// key must match the protected local identity before callers show this list.
export async function listBrowserMatrixDevices(userId, api) {
  if (!api || typeof api.get !== "function") throw new Error("Device service unavailable.");
  if ((await api.get("/matrix/device/identity")).data?.userId !== userId) throw new Error("The server identity changed. Device setup is blocked.");
  return withBrowserMatrixIdentity(userId, async (_machine, descriptor) => {
    const devices = (await api.get("/matrix/device/list")).data?.devices;
    if (!Array.isArray(devices) || devices.length > 100) throw new Error("Invalid device list.");
    const current = devices.filter(device => device?.current === true);
    if (current.length !== 1 || current[0].deviceId !== descriptor.deviceId || current[0].ed25519 !== descriptor.ed25519) throw new Error("The local device identity changed. Verification is blocked.");
    if (devices.some(device => typeof device?.deviceId !== "string" || !/^[A-Za-z0-9]{1,32}$/.test(device.deviceId) || !["authorized", "pending"].includes(device.serverAuthorization) || typeof device.ed25519 !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(device.ed25519) || typeof device.current !== "boolean")) throw new Error("Invalid device list.");
    return devices.map(device => ({ deviceId: device.deviceId, serverAuthorization: device.serverAuthorization, ed25519: device.ed25519, current: device.current, createdAt: device.createdAt }));
  });
}

// The exact key shown to the user must still match when the server performs
// the revocation; this cannot recover or erase keys already held remotely.
export async function revokeBrowserMatrixDevice(userId, target, api) {
  if (!target || target.current || typeof target.deviceId !== "string" || !/^[A-Za-z0-9]{1,32}$/.test(target.deviceId) || typeof target.ed25519 !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(target.ed25519) || typeof api?.post !== "function") throw new Error("Invalid device.");
  const devices = await listBrowserMatrixDevices(userId, api);
  const current = devices.find(device => device.current);
  if (current?.serverAuthorization !== "authorized" || !devices.some(device => device.deviceId === target.deviceId && device.ed25519 === target.ed25519 && !device.current)) throw new Error("The device list changed. Review it again.");
  const result = (await api.post("/matrix/device/revoke", { deviceId: current.deviceId, targetDeviceId: target.deviceId, expectedEd25519: target.ed25519 })).data;
  if (result?.revoked !== true || result.deviceId !== target.deviceId) throw new Error("Device removal was not confirmed.");
  return result;
}

// Resolve the same deterministic direct room on both accounts. Membership and
// room identity are verified before the local SDK's revision is updated.
export async function prepareBrowserDirectMatrixRoom(userId, peerAccountId, api) {
  const parsed = /^@([a-f0-9]{24}):([a-z0-9.-]{1,200})$/.exec(userId);
  if (!parsed || typeof peerAccountId !== "string" || !/^[a-f0-9]{24}$/.test(peerAccountId) || peerAccountId === parsed[1] || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Invalid direct conversation.");
  if ((await api.get("/matrix/device/identity")).data?.userId !== userId) throw new Error("The server identity changed. Sending is blocked.");
  const members = [userId, `@${peerAccountId}:${parsed[2]}`].sort();
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(["sonar-direct-v1", ...members]))));
  const roomId = `!${Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("")}:${parsed[2]}`;
  return withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
    const room = (await api.post("/matrix/messages/rooms/direct", { deviceId: descriptor.deviceId, peerAccountId })).data;
    if (room?.roomId !== roomId || room.kind !== "direct" || !Number.isSafeInteger(room.revision) || room.revision < 0 || !Array.isArray(room.members) || room.members.length !== 2 || [...room.members].sort().some((member, i) => member !== members[i])) throw new Error("The direct room identity changed. Sending is blocked.");
    const rotated = await ensureMatrixRoomRevision({ RoomId }, machine, descriptor.storeName, roomId, room.revision);
    return { roomId, revision: room.revision, members, kind: "direct", rotated };
  });
}

// Revisions are checked while holding the SDK store lock; callers must still
// query trusted recipient keys and safely deliver the newly generated session.
export async function syncBrowserMatrixRoom(userId, roomId, api) {
  if (!api || typeof api.get !== "function" || typeof roomId !== "string" || !/^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/.test(roomId)) throw new Error("Invalid room.");
  if ((await api.get("/matrix/device/identity")).data?.userId !== userId) throw new Error("The server identity changed. Sending is blocked.");
  return withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
    const snapshot = (await api.get("/matrix/device/room", { params: { roomId, deviceId: descriptor.deviceId } })).data;
    if (snapshot?.roomId !== roomId || !Number.isSafeInteger(snapshot.revision) || snapshot.revision < 0 || !["direct", "community"].includes(snapshot.kind) || !Array.isArray(snapshot.members) || snapshot.members.length < 2 || snapshot.members.length > 32 || new Set(snapshot.members).size !== snapshot.members.length || !snapshot.members.includes(userId) || snapshot.members.some(member => typeof member !== "string" || !/^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/.test(member))) throw new Error("Invalid room snapshot. Sending is blocked.");
    const rotated = await ensureMatrixRoomRevision({ RoomId }, machine, descriptor.storeName, roomId, snapshot.revision);
    return { roomId, revision: snapshot.revision, members: [...snapshot.members], kind: snapshot.kind, rotated };
  });
}
