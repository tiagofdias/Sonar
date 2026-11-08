// Receipts contain only a hash of an encrypted event, never its plaintext.
// The caller owns the SDK store lock. A failed acknowledgement leaves the
// receipt so a retry does not process the same Olm event twice.
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/, ID = /^[a-f0-9]{24}$/;
const MAX_AGE = 24 * 60 * 60 * 1000;
export class InterruptedVerification extends Error {
  constructor(eventId) { super("Verification processing was interrupted. Discard this event and start a new verification."); this.name = "InterruptedVerification"; this.eventId = eventId; }
}
let dbPromise;
function database() {
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-verification-inbox-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("receipts");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Verification inbox is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
async function operation(mode, callback) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("receipts", mode), store = tx.objectStore("receipts");
    let request; try { request = callback(store); } catch (error) { reject(error); return; }
    tx.oncomplete = () => resolve(request?.result);
    tx.onabort = () => reject(tx.error || new Error("Verification inbox storage failed."));
    tx.onerror = () => {};
  });
}
const keyFor = (store, id) => { if (!STORE.test(store || "") || !ID.test(id || "")) throw new Error("Invalid verification receipt."); return `${store}:${id}`; };
async function hash(event) {
  const bytes = new TextEncoder().encode(JSON.stringify(event));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}
export async function processVerificationInbox(machine, descriptor, rows, sdk, process) {
  if (!machine || typeof process !== "function" || !Array.isArray(rows) || rows.length > 100 || new Set(rows.map(row => row?.id)).size !== rows.length) throw new Error("Invalid verification inbox.");
  const acknowledged = [];
  for (const row of rows) {
    const key = keyFor(descriptor.storeName, row?.id), digest = await hash(row.event), saved = await operation("readonly", store => store.get(key));
    if (saved) {
      if (saved.digest !== digest) throw new Error("Verification event changed.");
      if (saved.state !== "processed") throw new InterruptedVerification(row.id);
    }
    else {
      // This marker is committed before the SDK call. The SDK and this store
      // cannot share a transaction, so an interruption is ambiguous and must
      // stop rather than silently replay or trust the verification.
      await operation("readwrite", store => store.add({ digest, state: "processing", createdAt: Date.now() }, key));
      const lists = new sdk.DeviceLists(); let events;
      try {
        events = await machine.receiveSyncChangesMsc4186(JSON.stringify([row.event]), lists, new Map());
        if (!Array.isArray(events) || events.length !== 1 || events[0].type !== sdk.ProcessedToDeviceEventType.Decrypted) throw new Error("Verification event could not be decrypted.");
        await process(events[0], row.event);
        await operation("readwrite", store => store.put({ digest, state: "processed", createdAt: Date.now() }, key));
      } finally { events?.forEach(event => event?.free()); lists.free(); }
    }
    acknowledged.push(row.id);
  }
  return acknowledged;
}
export async function isInterruptedVerificationReceipt(descriptor, row) {
  const saved = await operation("readonly", store => store.get(keyFor(descriptor.storeName, row?.id)));
  return Boolean(saved && saved.state === "processing" && saved.digest === await hash(row.event));
}
export async function clearVerificationReceipts(descriptor, ids) {
  if (!Array.isArray(ids) || ids.length > 100) throw new Error("Invalid verification receipt.");
  await operation("readwrite", store => { for (const id of ids) store.delete(keyFor(descriptor.storeName, id)); });
}
export async function purgeExpiredVerificationReceipts() {
  const now = Date.now();
  await operation("readwrite", store => {
    const request = store.openCursor();
    request.onsuccess = () => { const cursor = request.result; if (!cursor) return; if (!Number.isFinite(cursor.value?.createdAt) || now - cursor.value.createdAt > MAX_AGE) cursor.delete(); cursor.continue(); };
  });
}
