// Experimental outgoing Megolm rotation. The caller must hold the Matrix
// store's exclusive lock before calling this function and must obtain the
// revision through an authenticated, room-authorized server request.
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/;
const ROOM = /^![A-Za-z0-9_-]{1,100}:[a-z0-9.-]{1,200}$/;
let dbPromise;
function database() {
  if (!globalThis.indexedDB || !globalThis.navigator?.locks) throw new Error("Secure local storage is unavailable.");
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-room-revisions-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("revisions");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Room revision storage is blocked."));
  }).catch(error => { dbPromise = undefined; throw error; });
}
async function read(key) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const request = db.transaction("revisions").objectStore("revisions").get(key);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
async function write(key, revision) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("revisions", "readwrite");
    transaction.objectStore("revisions").put(revision, key);
    transaction.oncomplete = resolve;
    transaction.onabort = () => reject(transaction.error || new Error("Could not save room revision."));
    transaction.onerror = () => {};
  });
}

// Invalidate before saving the revision. If the browser closes between those
// steps, the next call invalidates again rather than reusing an old key.
export async function ensureMatrixRoomRevision(sdk, machine, storeName, roomId, revision) {
  if (!sdk?.RoomId || !machine || typeof machine.invalidateGroupSession !== "function" || typeof storeName !== "string" || !STORE.test(storeName) || typeof roomId !== "string" || !ROOM.test(roomId) || !Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid room revision.");
  const key = `${storeName}:${roomId}`;
  await database();
  return navigator.locks.request(`matrix-room-revision:${key}`, { mode: "exclusive" }, async () => {
    const previous = await read(key);
    if (previous !== undefined && (!Number.isSafeInteger(previous) || previous < 0 || revision < previous)) throw new Error("Room revision changed unexpectedly. Sending is blocked.");
    if (previous === revision) return false;
    const room = new sdk.RoomId(roomId);
    try { await machine.invalidateGroupSession(room); }
    finally { room.free(); }
    await write(key, revision);
    return true;
  });
}
