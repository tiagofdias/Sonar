// Browser-local protection for the experimental Matrix store. A non-extractable
// WebCrypto key wraps the SDK store key in IndexedDB. This protects against
// accidental disclosure, not malicious same-origin scripts or browser access.
// It neither exports the legacy identity nor provides cross-device recovery.
import { createMatrixDeviceStore, withMatrixDeviceStore } from "./matrixStore.js";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/;
let dbPromise;
function database() {
  if (!globalThis.indexedDB || !globalThis.navigator?.locks || !globalThis.crypto?.subtle) throw new Error("Secure local storage is unavailable.");
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-identity-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("identities");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Identity storage is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
function validate(userId) {
  if (typeof userId !== "string" || !USER.test(userId)) throw new Error("Invalid Matrix identity.");
}
const aad = descriptor => new TextEncoder().encode(JSON.stringify(["sonar-matrix-local-v1", descriptor.userId, descriptor.deviceId, descriptor.storeName, descriptor.ed25519]));
async function read(userId) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const request = db.transaction("identities").objectStore("identities").get(userId);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
async function insert(userId, record) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("identities", "readwrite");
    tx.objectStore("identities").add(record, userId);
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error || new Error("Identity storage failed."));
    tx.onerror = () => {};
  });
}
function validateRecord(userId, record) {
  const d = record?.descriptor;
  if (record?.version !== 1 || !d || d.userId !== userId || typeof d.deviceId !== "string" || !DEVICE.test(d.deviceId) || typeof d.storeName !== "string" || !STORE.test(d.storeName) || typeof d.ed25519 !== "string" || !/^[A-Za-z0-9+/]{43}$/.test(d.ed25519) || !record.wrappingKey || !(record.iv instanceof Uint8Array) || record.iv.length !== 12 || !(record.wrappedKey instanceof ArrayBuffer)) throw new Error("Matrix identity is damaged or unavailable.");
  return d;
}

// Explicit setup only. A second setup attempt fails instead of replacing an
// existing record. Locks span SDK store creation and the atomic insert.
export async function createBrowserMatrixIdentity(userId, deviceId) {
  validate(userId);
  if (typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid Matrix device identity.");
  await database();
  return navigator.locks.request(`matrix-identity:${userId}`, { mode: "exclusive" }, async () => {
    if (await read(userId) !== undefined) throw new Error("A Matrix identity already exists in this browser.");
    const raw = crypto.getRandomValues(new Uint8Array(32));
    try {
      const descriptor = await createMatrixDeviceStore(userId, deviceId, raw);
      const wrappingKey = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
      const iv = crypto.getRandomValues(new Uint8Array(12));
      const wrappedKey = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad(descriptor) }, wrappingKey, raw);
      await insert(userId, { version: 1, descriptor, wrappingKey, iv, wrappedKey });
      return descriptor;
    } finally { raw.fill(0); }
  });
}

// A missing record is distinct from a damaged one. This never opens or creates
// an SDK store; callers must still use withBrowserMatrixIdentity to validate it.
export async function browserMatrixIdentityExists(userId) {
  validate(userId);
  await database();
  return navigator.locks.request(`matrix-identity:${userId}`, { mode: "exclusive" }, async () => {
    const record = await read(userId);
    if (record === undefined) return false;
    validateRecord(userId, record);
    return true;
  });
}

// Missing or corrupted data never falls back to creating a new identity.
export async function withBrowserMatrixIdentity(userId, callback) {
  validate(userId);
  if (typeof callback !== "function") throw new Error("Invalid Matrix operation.");
  await database();
  return navigator.locks.request(`matrix-identity:${userId}`, { mode: "exclusive" }, async () => {
    const record = await read(userId);
    if (!record) throw new Error("No Matrix identity exists in this browser.");
    const descriptor = validateRecord(userId, record);
    let raw;
    try {
      raw = new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: record.iv, additionalData: aad(descriptor) }, record.wrappingKey, record.wrappedKey));
      if (raw.length !== 32) throw new Error("Invalid key length.");
    } catch { raw?.fill(0); throw new Error("Matrix identity is damaged or unavailable."); }
    try { return await withMatrixDeviceStore(descriptor, raw, machine => callback(machine, descriptor)); }
    finally { raw.fill(0); }
  });
}
