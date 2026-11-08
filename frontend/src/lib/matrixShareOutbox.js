// Ciphertext-only room-key outbox. The caller must hold the SDK store lock;
// requests are saved before network delivery and reused unchanged on retry.
import { CollectStrategy, EncryptionSettings, RequestType, RoomId, UserId } from "@matrix-org/matrix-sdk-crypto-wasm";
import { establishTrustedRoomSessions, requireTrustedRoomDevices } from "./matrixRoomKeys.js";
import { ensureMatrixRoomRevision } from "./matrixRoomRevision.js";
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/, TXN = /^[A-Za-z0-9._~-]{1,128}$/;
let dbPromise;
function database() {
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-share-outbox-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("shares");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Room key outbox is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
async function transact(mode, callback) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("shares", mode), store = tx.objectStore("shares");
    let request; try { request = callback(store); } catch (error) { reject(error); return; }
    tx.oncomplete = () => resolve(request?.result);
    tx.onabort = () => reject(tx.error || new Error("Room key outbox failed."));
    tx.onerror = () => {};
  });
}
function validateDescriptor(descriptor, room) {
  if (!STORE.test(descriptor?.storeName || "") || !room || typeof room.roomId !== "string" || !Number.isSafeInteger(room.revision) || !Array.isArray(room.members)) throw new Error("Invalid room key outbox.");
}
async function pending(descriptor, room) {
  validateDescriptor(descriptor, room);
  return (await transact("readonly", store => store.getAll())).filter(row => row?.storeName === descriptor.storeName && row.userId === descriptor.userId && row.deviceId === descriptor.deviceId && row.roomId === room.roomId);
}
export async function pendingRoomKeyShareState(descriptor, room) {
  const rows = await pending(descriptor, room);
  return { preparing: rows.some(row => row.kind === "preparing"), count: rows.filter(row => row.kind !== "preparing").length };
}
async function checkSnapshot(descriptor, room, api) {
  const remote = (await api.get("/matrix/device/room", { params: { roomId: room.roomId, deviceId: descriptor.deviceId } })).data;
  if (remote?.roomId !== room.roomId || remote.revision !== room.revision || remote.kind !== room.kind || !Array.isArray(remote.members) || remote.members.length !== room.members.length || [...remote.members].sort().some((user, index) => user !== [...room.members].sort()[index])) throw new Error("Room membership changed. Key sharing is blocked.");
}
async function checkRoom(machine, descriptor, room, api) {
  await checkSnapshot(descriptor, room, api);
  return requireTrustedRoomDevices(machine, descriptor, room, api);
}
function validateMessages(messages, trusted, senderKey) {
  if (!messages || typeof messages !== "object" || Array.isArray(messages)) throw new Error("Invalid encrypted key share.");
  let count = 0;
  for (const [userId, devices] of Object.entries(messages)) {
    if (!devices || typeof devices !== "object" || Array.isArray(devices)) throw new Error("Invalid encrypted key share.");
    for (const [deviceId, content] of Object.entries(devices)) {
      const device = trusted.find(item => item.userId === userId && item.deviceId === deviceId);
      if (!device || content?.algorithm !== "m.olm.v1.curve25519-aes-sha2" || content.sender_key !== senderKey || !content.ciphertext || typeof content.ciphertext !== "object" || Array.isArray(content.ciphertext) || Object.keys(content.ciphertext).length !== 1 || !Object.hasOwn(content.ciphertext, device.curve25519)) throw new Error("Key share recipient changed. Sending is blocked.");
      count++;
    }
  }
  if (!count || count > 100) throw new Error("Invalid encrypted key share.");
}
export async function prepareTrustedRoomKeyShares(machine, descriptor, room, api) {
  validateDescriptor(descriptor, room);
  if (typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Room service unavailable.");
  const previous = await pending(descriptor, room), markerKey = `${descriptor.storeName}:prepare:${room.roomId}`;
  if (previous.some(row => row.kind !== "preparing")) throw new Error("Deliver pending room keys before creating another share.");
  if (previous.length) {
    // The browser stopped while the SDK could have advanced its ratchet but
    // before the encrypted requests were committed. Never reuse that session.
    const id = new RoomId(room.roomId);
    try { await machine.invalidateGroupSession(id); } finally { id.free(); }
    await transact("readwrite", store => store.delete(markerKey));
  }
  await checkRoom(machine, descriptor, room, api);
  await ensureMatrixRoomRevision({ RoomId }, machine, descriptor.storeName, room.roomId, room.revision);
  const { trusted } = await establishTrustedRoomSessions(machine, descriptor, room, api);
  const keys = machine.identityKeys, curve = keys.curve25519;
  let shares, settings;
  try {
    await transact("readwrite", store => store.add({ kind: "preparing", storeName: descriptor.storeName, userId: descriptor.userId, deviceId: descriptor.deviceId, roomId: room.roomId, revision: room.revision }, markerKey));
    settings = new EncryptionSettings(); settings.sharingStrategy = CollectStrategy.onlyTrustedDevices();
    shares = await machine.shareRoomKey(new RoomId(room.roomId), room.members.map(user => new UserId(user)), settings);
    const rows = shares.map(request => {
      if (request.type !== RequestType.ToDevice || request.event_type !== "m.room.encrypted" || typeof request.id !== "string" || !TXN.test(request.id) || request.txn_id !== request.id) throw new Error("Invalid SDK key share.");
      let body; try { body = JSON.parse(request.body); } catch { throw new Error("Invalid SDK key share."); }
      validateMessages(body?.messages, trusted, curve.toBase64());
      return { id: request.id, storeName: descriptor.storeName, userId: descriptor.userId, deviceId: descriptor.deviceId, roomId: room.roomId, revision: room.revision, eventType: request.event_type, messages: body.messages };
    });
    if (rows.length > 100) throw new Error("Too many key shares.");
    await transact("readwrite", store => { for (const row of rows) store.add(row, `${descriptor.storeName}:${row.id}`); store.delete(markerKey); });
    return rows.length;
  } finally { settings?.free(); curve.free(); keys.free(); }
}
export async function deliverTrustedRoomKeyShares(machine, descriptor, room, api) {
  const rows = await pending(descriptor, room);
  if (!rows.length) return 0;
  if (rows.some(row => row.kind === "preparing")) throw new Error("Room key preparation was interrupted. Prepare it again.");
  const trusted = await checkRoom(machine, descriptor, room, api);
  const keys = machine.identityKeys, curve = keys.curve25519;
  try {
    for (const row of rows) {
      if (row.revision !== room.revision || !TXN.test(row.id || "") || row.eventType !== "m.room.encrypted") throw new Error("Pending room key is stale.");
      validateMessages(row.messages, trusted, curve.toBase64());
      const response = (await api.post("/matrix/messages/shares/send", { roomId: row.roomId, revision: row.revision, deviceId: row.deviceId, transactionId: row.id, eventType: row.eventType, messages: row.messages })).data;
      if (response?.accepted !== true) throw new Error("Room key delivery was not confirmed.");
      await machine.markRequestAsSent(row.id, RequestType.ToDevice, "{}");
      await transact("readwrite", store => store.delete(`${descriptor.storeName}:${row.id}`));
    }
  } finally { curve.free(); keys.free(); }
  return rows.length;
}

// Explicitly discard only shares from older revisions after confirming the
// current snapshot. Invalidate before deletion so interruption cannot reuse a
// group session that might already have reached a removed device.
export async function discardStaleRoomKeyShares(machine, descriptor, room, api) {
  validateDescriptor(descriptor, room);
  if (typeof api?.get !== "function") throw new Error("Room service unavailable.");
  await checkSnapshot(descriptor, room, api);
  const rows = await pending(descriptor, room);
  if (rows.some(row => !Number.isSafeInteger(row.revision) || row.revision < 0 || row.revision > room.revision || (row.kind !== "preparing" && !TXN.test(row.id || "")))) throw new Error("Invalid pending room key.");
  const stale = rows.filter(row => row.revision < room.revision);
  if (!stale.length) return 0;
  await ensureMatrixRoomRevision({ RoomId }, machine, descriptor.storeName, room.roomId, room.revision);
  const id = new RoomId(room.roomId);
  try { await machine.invalidateGroupSession(id); } finally { id.free(); }
  await transact("readwrite", store => { for (const row of stale) store.delete(row.kind === "preparing" ? `${descriptor.storeName}:prepare:${room.roomId}` : `${descriptor.storeName}:${row.id}`); });
  return stale.length;
}
