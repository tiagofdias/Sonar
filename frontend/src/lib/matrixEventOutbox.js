// Ciphertext-only durable outbox. All functions require the Matrix SDK store
// lock. Retrying always sends the same ciphertext and server transaction.
import { requireTrustedRoomDevices } from "./matrixRoomKeys.js";
import { RoomId } from "@matrix-org/matrix-sdk-crypto-wasm";
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/, ID = /^[a-f0-9]{32}$/, TOKEN = /^[A-Za-z0-9_-]{43}$/;
let dbPromise;
function database() {
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-event-outbox-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("events");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Message outbox is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
async function storage(mode, callback) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("events", mode), store = tx.objectStore("events");
    let request; try { request = callback(store); } catch (error) { reject(error); return; }
    tx.oncomplete = () => resolve(request?.result);
    tx.onabort = () => reject(tx.error || new Error("Message outbox failed."));
    tx.onerror = () => {};
  });
}
const key = (storeName, id) => {
  if (!STORE.test(storeName || "") || !ID.test(id || "")) throw new Error("Invalid outbox item.");
  return `${storeName}:${id}`;
};
async function checkRoom(machine, descriptor, room, api) {
  const remote = (await api.get("/matrix/device/room", { params: { roomId: room.roomId, deviceId: descriptor.deviceId } })).data;
  if (remote?.roomId !== room.roomId || remote.revision !== room.revision || remote.kind !== room.kind || !Array.isArray(remote.members) || remote.members.length !== room.members.length || [...remote.members].sort().some((member, index) => member !== [...room.members].sort()[index])) throw new Error("Room membership changed. Sending is blocked.");
  await requireTrustedRoomDevices(machine, descriptor, room, api);
}
export async function queueEncryptedRoomEvent(machine, descriptor, room, content, api, attachment = null) {
  if (!machine || !STORE.test(descriptor?.storeName || "") || typeof api?.get !== "function" || typeof api?.post !== "function" || !content || typeof content !== "object" || Array.isArray(content)) throw new Error("Invalid encrypted message.");
  if (attachment !== null && (!attachment || !/^[a-f0-9]{32}$/.test(attachment.id || "") || !(attachment.ciphertext instanceof Uint8Array) || attachment.ciphertext.byteLength < 1 || attachment.ciphertext.byteLength > 10 * 1024 * 1024)) throw new Error("Invalid encrypted attachment.");
  const fields = ["algorithm", "ciphertext", "device_id", "sender_key", "session_id"];
  const keys = machine.identityKeys, curve = keys.curve25519;
  try {
    if (Object.keys(content).length !== fields.length || !fields.every(field => Object.hasOwn(content, field)) || content.algorithm !== "m.megolm.v1.aes-sha2" || content.device_id !== descriptor.deviceId || content.sender_key !== curve.toBase64() || typeof content.session_id !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(content.session_id) || typeof content.ciphertext !== "string" || !/^[A-Za-z0-9+/]{24,48000}$/.test(content.ciphertext)) throw new Error("Invalid encrypted message.");
  } finally { curve.free(); keys.free(); }
  await checkRoom(machine, descriptor, room, api);
  const scope = { roomId: room.roomId, revision: room.revision, deviceId: descriptor.deviceId };
  const transaction = (await api.post("/matrix/messages/events/transaction", scope)).data;
  if (typeof transaction?.token !== "string" || !TOKEN.test(transaction.token) || !Number.isFinite(Date.parse(transaction.expiresAt)) || Date.parse(transaction.expiresAt) <= Date.now()) throw new Error("Invalid message transaction.");
  const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, "0")).join("");
  await storage("readwrite", store => store.add({ id, storeName: descriptor.storeName, userId: descriptor.userId, ...scope, token: transaction.token, expiresAt: transaction.expiresAt, content, ...(attachment ? { attachment } : {}) }, key(descriptor.storeName, id)));
  return id;
}
export async function pendingEncryptedRoomEvents(descriptor) {
  if (!STORE.test(descriptor?.storeName || "")) throw new Error("Invalid device identity.");
  const rows = await storage("readonly", store => store.getAll());
  return rows.filter(row => row?.storeName === descriptor.storeName && row.userId === descriptor.userId && row.deviceId === descriptor.deviceId).map(row => ({ id: row.id, roomId: row.roomId, revision: row.revision, expiresAt: row.expiresAt }));
}
export async function deliverEncryptedRoomEvent(machine, descriptor, room, id, api) {
  const itemKey = key(descriptor?.storeName, id), item = await storage("readonly", store => store.get(itemKey));
  if (!item || item.id !== id || item.storeName !== descriptor.storeName || item.userId !== descriptor.userId || item.deviceId !== descriptor.deviceId || item.roomId !== room?.roomId || item.revision !== room.revision || !TOKEN.test(item.token || "") || !Number.isFinite(Date.parse(item.expiresAt)) || Date.parse(item.expiresAt) <= Date.now()) throw new Error("Pending message unavailable or expired.");
  await checkRoom(machine, descriptor, room, api);
  const envelope = { roomId: item.roomId, revision: item.revision, deviceId: item.deviceId, token: item.token, content: item.content };
  let response;
  if (item.attachment) {
    if (!/^[a-f0-9]{32}$/.test(item.attachment.id || "") || !(item.attachment.ciphertext instanceof Uint8Array) || item.attachment.ciphertext.byteLength < 1 || item.attachment.ciphertext.byteLength > 10 * 1024 * 1024) throw new Error("Pending attachment is invalid.");
    const header = new TextEncoder().encode(JSON.stringify({ ...envelope, attachmentId: item.attachment.id }));
    if (header.length > 65536) throw new Error("Pending attachment is invalid.");
    const prefix = new Uint8Array(4); new DataView(prefix.buffer).setUint32(0, header.length);
    const frame = new Blob([prefix, header, item.attachment.ciphertext], { type: "application/octet-stream" });
    response = await api.post("/matrix/messages/events/send-attachment", frame, { headers: { "Content-Type": "application/octet-stream" } });
  } else response = await api.post("/matrix/messages/events/send", envelope);
  const result = response.data;
  if (typeof result?._id !== "string" || !/^[a-f0-9]{24}$/.test(result._id) || (!result.unavailable && (!result.content || Object.keys(result.content).length !== 5 || Object.keys(item.content).some(field => result.content[field] !== item.content[field])))) throw new Error("Message delivery was not confirmed.");
  if (item.attachment && !result.unavailable && result.attachmentId !== item.attachment.id) throw new Error("Attachment delivery was not confirmed.");
  await storage("readwrite", store => store.delete(itemKey));
  return { id: result._id, duplicate: result.duplicate === true, unavailable: result.unavailable === true };
}

// The server may already have accepted the event even if confirmation was lost.
// Rotate before removing it so later messages never reuse an uncertain session.
export async function discardEncryptedRoomEvent(machine, descriptor, room, id) {
  const itemKey = key(descriptor?.storeName, id), item = await storage("readonly", store => store.get(itemKey));
  if (!machine || !item || item.id !== id || item.storeName !== descriptor.storeName || item.userId !== descriptor.userId || item.deviceId !== descriptor.deviceId || item.roomId !== room?.roomId) throw new Error("Pending message unavailable.");
  const roomId = new RoomId(room.roomId);
  try { await machine.invalidateGroupSession(roomId); } finally { roomId.free(); }
  await storage("readwrite", store => store.delete(itemKey));
}
