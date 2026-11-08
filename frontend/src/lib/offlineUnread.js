// Local read positions; no message text, keys, or read receipt is stored here.
const ID = /^[a-f0-9]{24}$/;
let dbPromise;
function db() {
  if (!globalThis.indexedDB) return Promise.reject(new Error("Local storage unavailable"));
  return dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-unread-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("positions");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  }).catch(error => { dbPromise = undefined; throw error; });
}
function position(value) {
  const time = typeof value?.createdAt === "string" ? Date.parse(value.createdAt) : NaN;
  if (!Number.isFinite(time) || typeof value?.messageId !== "string" || !ID.test(value.messageId)) throw new Error("Invalid message position");
  return { createdAt: new Date(time).toISOString(), messageId: value.messageId };
}
export function newer(left, right) {
  return !right || left.createdAt > right.createdAt || (left.createdAt === right.createdAt && left.messageId > right.messageId);
}
function key(userId, peerId) {
  if (typeof userId !== "string" || !ID.test(userId) || typeof peerId !== "string" || !ID.test(peerId)) throw new Error("Invalid account or contact");
  return `${userId}:${peerId}`;
}
function channelKey(userId, communityId, channelId) {
  if (![userId, communityId, channelId].every(id => typeof id === "string" && ID.test(id))) throw new Error("Invalid channel identity");
  return `${userId}:channel:${communityId}:${channelId}`;
}
export async function readPosition(userId, peerId) {
  return readKey(key(userId, peerId));
}
async function readKey(id) {
  const database = await db();
  return new Promise((resolve, reject) => {
    const request = database.transaction("positions").objectStore("positions").get(id);
    request.onsuccess = () => resolve(request.result || null);
    request.onerror = () => reject(request.error);
  });
}
export async function rememberPosition(userId, peerId, value) {
  return rememberKey(key(userId, peerId), value);
}
async function rememberKey(id, value) {
  const next = position(value), database = await db();
  return new Promise((resolve, reject) => {
    const transaction = database.transaction("positions", "readwrite");
    const store = transaction.objectStore("positions"), request = store.get(id);
    request.onsuccess = () => { if (newer(next, request.result)) store.put(next, id); };
    transaction.oncomplete = () => resolve();
    transaction.onabort = () => reject(transaction.error || new Error("Could not store read position"));
    transaction.onerror = () => {};
  });
}
export function readChannelPosition(userId, communityId, channelId) { return readKey(channelKey(userId, communityId, channelId)); }
export function rememberChannelPosition(userId, communityId, channelId, value) { return rememberKey(channelKey(userId, communityId, channelId), value); }
export function candidatePosition(candidate) { return position(candidate); }
