// Persist acknowledgement receipts after SDK processing so a lost HTTP
// response never makes us decrypt the same Olm event a second time.
import { DeviceLists, ProcessedToDeviceEventType } from "@matrix-org/matrix-sdk-crypto-wasm";
import { requireTrustedRoomDevices } from "./matrixRoomKeys.js";
const ID = /^[a-f0-9]{24}$/;
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/;
let databasePromise;
function database() {
  return databasePromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-matrix-inbox-receipts-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("receipts");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Inbox storage is blocked."));
  }).catch(error => { databasePromise = undefined; throw error; });
}
async function receipt(key, value, remove = false) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("receipts", value === undefined && !remove ? "readonly" : "readwrite"), store = tx.objectStore("receipts");
    let result;
    if (remove) store.delete(key);
    else if (value !== undefined) store.put(value, key);
    else { const request = store.get(key); request.onsuccess = () => { result = request.result; }; }
    tx.oncomplete = () => resolve(result);
    tx.onabort = () => reject(tx.error || new Error("Inbox storage failed."));
    tx.onerror = () => {};
  });
}
async function eventHash(event) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(event))));
  return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}

// The caller must hold the protected SDK store lock. Unknown or unverified
// senders and malformed events are not acknowledged or exposed as messages.
export async function receiveTrustedRoomKeys(machine, descriptor, room, api) {
  if (!machine || !USER.test(descriptor?.userId || "") || !DEVICE.test(descriptor?.deviceId || "") || !STORE.test(descriptor?.storeName || "") || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Invalid room inbox.");
  const rows = (await api.get("/matrix/messages/shares/inbox", { params: { deviceId: descriptor.deviceId, roomId: room?.roomId } })).data?.events;
  if (!Array.isArray(rows) || rows.length > 100 || new Set(rows.map(row => row?.id)).size !== rows.length || rows.some(row => typeof row?.id !== "string" || !ID.test(row.id) || row.event?.type !== "m.room.encrypted" || !USER.test(row.event?.sender || "") || !row.event?.content || typeof row.event.content !== "object" || Array.isArray(row.event.content))) throw new Error("Invalid room inbox.");
  const trusted = await requireTrustedRoomDevices(machine, descriptor, room, api);
  const acknowledged = [];
  for (const row of rows) {
    const key = `${descriptor.storeName}:${row.id}`, digest = await eventHash(row.event), saved = await receipt(key);
    if (saved) {
      if (saved !== digest) throw new Error("A queued room key changed. Processing is blocked.");
    } else {
      const lists = new DeviceLists();
      let events;
      try {
        events = await machine.receiveSyncChangesMsc4186(JSON.stringify([row.event]), lists, new Map());
        if (!Array.isArray(events) || events.length !== 1 || events[0].type !== ProcessedToDeviceEventType.Decrypted) throw new Error("Room key could not be decrypted.");
        let plaintext;
        try { plaintext = JSON.parse(events[0].rawEvent); } catch { throw new Error("Invalid room key event."); }
        if (plaintext?.type !== "m.room_key" || plaintext?.content?.room_id !== room.roomId) throw new Error("Room key belongs to a different room.");
        const info = events[0].encryptionInfo, sender = info?.sender;
        try {
          if (!info || !sender || sender.toString() !== row.event.sender || !trusted.some(device => device.userId === row.event.sender && device.curve25519 === info.senderCurve25519Key)) throw new Error("Room key sender is not verified.");
        } finally { sender?.free(); info?.free(); }
        await receipt(key, digest);
      } finally { events?.forEach(event => event?.free()); lists.free(); }
    }
    acknowledged.push({ key, id: row.id });
  }
  if (acknowledged.length) {
    const result = (await api.post("/matrix/messages/shares/ack", { deviceId: descriptor.deviceId, roomId: room.roomId, ids: acknowledged.map(row => row.id) })).data;
    if (result?.acknowledged !== true) throw new Error("Room key acknowledgement failed.");
    for (const row of acknowledged) await receipt(row.key, undefined, true);
  }
  return acknowledged.length;
}
