// Durable ciphertext-only outbox for an experimental Matrix SAS flow. This is
// not a server API or a guarantee that the SDK can resume a SAS after reload.
import { prepareVerificationDelivery, sendPreparedVerification } from "./matrixVerification.js";

const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/;
const TXN = /^[A-Za-z0-9._~-]{1,128}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const MAX_AGE = 10 * 60 * 1000;
let dbPromise;
function database() {
  if (!globalThis.indexedDB || !globalThis.navigator?.locks || !globalThis.crypto?.subtle) throw new Error("Secure local storage is unavailable.");
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-verification-outbox-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("pending");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Verification storage is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
function keyFor(storeName, id) {
  if (typeof storeName !== "string" || !STORE.test(storeName) || typeof id !== "string" || !TXN.test(id)) throw new Error("Invalid verification identity.");
  return `${storeName}:${id}`;
}
async function read(key) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const request = db.transaction("pending").objectStore("pending").get(key);
    request.onsuccess = () => resolve(request.result || null);
    request.onerror = () => reject(request.error);
  });
}
async function write(key, value, remove = false) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("pending", "readwrite");
    if (remove) transaction.objectStore("pending").delete(key);
    else transaction.objectStore("pending").add(value, key);
    transaction.oncomplete = () => resolve();
    transaction.onabort = () => reject(transaction.error || new Error("Verification storage failed."));
    transaction.onerror = () => {};
  });
}
async function digest(request) {
  if (typeof request?.body !== "string" || request.body.length > 32768 || typeof request.event_type !== "string" || request.event_type.length > 100) throw new Error("Invalid verification request.");
  const bytes = new TextEncoder().encode(JSON.stringify([request.event_type, request.body]));
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(hash, value => value.toString(16).padStart(2, "0")).join("");
}
function checkAge(record) {
  const age = Date.now() - record.createdAt;
  if (!Number.isFinite(age) || age < 0 || age > MAX_AGE) throw new Error("Verification expired. Start again.");
}

// The caller must already hold the corresponding Matrix store lock. The
// additional per-request lock serializes outbox access across browser tabs.
export async function queueVerification(sdk, machine, storeName, userId, deviceId, request) {
  const key = keyFor(storeName, request?.id);
  if (!sdk?.RequestType || request?.type !== sdk.RequestType.ToDevice || request.id !== request.txn_id || typeof userId !== "string" || !USER.test(userId) || typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid recipient or verification request.");
  const requestDigest = await digest(request);
  return navigator.locks.request(`outbox:${key}`, { mode: "exclusive" }, async () => {
    const existing = await read(key);
    if (existing) {
      checkAge(existing);
      if (existing.digest !== requestDigest || existing.userId !== userId || existing.deviceId !== deviceId) throw new Error("Verification transaction changed.");
      return existing.prepared;
    }
    const prepared = await prepareVerificationDelivery(sdk, machine, userId, deviceId, request);
    await write(key, { createdAt: Date.now(), userId, deviceId, digest: requestDigest, prepared });
    return prepared;
  });
}

// `send` must reject on failure and return true only after durable acceptance.
// The same encrypted object stays on disk on any failure, including lost ACKs.
export async function deliverQueuedVerification(sdk, machine, storeName, requestId, send) {
  const key = keyFor(storeName, requestId);
  return navigator.locks.request(`outbox:${key}`, { mode: "exclusive" }, async () => {
    const record = await read(key);
    if (!record) throw new Error("Pending verification not found.");
    checkAge(record);
    await sendPreparedVerification(sdk, machine, record.prepared, send);
    await write(key, null, true);
  });
}

// Expired requests must never be resent; deleting one does not verify a device.
export async function discardExpiredVerification(storeName, requestId) {
  const key = keyFor(storeName, requestId);
  return navigator.locks.request(`outbox:${key}`, { mode: "exclusive" }, async () => {
    const record = await read(key);
    if (!record) return false;
    const age = Date.now() - record.createdAt;
    if (Number.isFinite(age) && age >= 0 && age <= MAX_AGE) throw new Error("Verification is still active.");
    await write(key, null, true);
    return true;
  });
}
