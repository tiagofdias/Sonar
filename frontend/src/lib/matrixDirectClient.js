// Short-lived, store-locked operations for the opt-in Matrix direct view.
import { prepareBrowserDirectMatrixRoom } from "./matrixDeviceClient.js";
import { withBrowserMatrixIdentity } from "./matrixIdentityVault.js";
import { receiveTrustedRoomKeys } from "./matrixRoomInbox.js";
import { readTrustedRoomHistory } from "./matrixRoomHistory.js";
import { sendTrustedRoomText, sendTrustedRoomReaction, sendTrustedRoomAttachment } from "./matrixRoomSend.js";
import { decryptMatrixAttachment } from "./matrixAttachment.js";
import { requireTrustedRoomDevices } from "./matrixRoomKeys.js";
import { deliverEncryptedRoomEvent, discardEncryptedRoomEvent, pendingEncryptedRoomEvents } from "./matrixEventOutbox.js";

async function inRoom(accountId, peerId, api, callback) {
  if (!/^[a-f0-9]{24}$/.test(accountId || "") || !/^[a-f0-9]{24}$/.test(peerId || "") || typeof callback !== "function") throw new Error("Invalid conversation.");
  const userId = (await api.get("/matrix/device/identity")).data?.userId;
  if (typeof userId !== "string" || !userId.startsWith(`@${accountId}:`)) throw new Error("Account identity changed.");
  const room = await prepareBrowserDirectMatrixRoom(userId, peerId, api);
  return withBrowserMatrixIdentity(userId, (machine, descriptor) => callback(machine, descriptor, room));
}
export async function readDirectMatrixHistory(accountId, peerId, api, options = {}) {
  return inRoom(accountId, peerId, api, async (machine, descriptor, room) => {
    await receiveTrustedRoomKeys(machine, descriptor, room, api);
    const history = await readTrustedRoomHistory(machine, descriptor, room, api, options);
    const pending = (await pendingEncryptedRoomEvents(descriptor)).filter(item => item.roomId === room.roomId);
    return { ...history, pending };
  });
}
export async function sendDirectMatrixText(accountId, peerId, text, api, replyTo = null) {
  return inRoom(accountId, peerId, api, (machine, descriptor, room) => sendTrustedRoomText(machine, descriptor, room, text, api, replyTo));
}
export async function sendDirectMatrixReaction(accountId, peerId, target, emoji, api) {
  return inRoom(accountId, peerId, api, (machine, descriptor, room) => sendTrustedRoomReaction(machine, descriptor, room, target, emoji, api));
}
export async function sendDirectMatrixAttachment(accountId, peerId, file, api) {
  return inRoom(accountId, peerId, api, (machine, descriptor, room) => sendTrustedRoomAttachment(machine, descriptor, room, file, api));
}
export async function downloadDirectMatrixAttachment(accountId, peerId, attachment, api) {
  if (!attachment || !/^[a-f0-9]{32}$/.test(attachment.id || "") || !Number.isSafeInteger(attachment.size) || attachment.size < 1 || attachment.size > 10 * 1024 * 1024 || typeof attachment.metadata !== "string" || attachment.metadata.length > 4096) throw new Error("Invalid attachment.");
  return inRoom(accountId, peerId, api, async (machine, descriptor, room) => {
    await requireTrustedRoomDevices(machine, descriptor, room, api);
    const response = await api.get("/matrix/messages/events/attachment", { params: { roomId: room.roomId, deviceId: descriptor.deviceId, attachmentId: attachment.id }, responseType: "arraybuffer" });
    const ciphertext = new Uint8Array(response.data);
    if (ciphertext.byteLength !== attachment.size) throw new Error("Attachment size mismatch.");
    const plaintext = decryptMatrixAttachment(ciphertext, attachment.metadata);
    if (plaintext.byteLength !== attachment.size) { plaintext.fill(0); throw new Error("Attachment size mismatch."); }
    return plaintext;
  });
}
export async function retryDirectMatrixText(accountId, peerId, id, api) {
  return inRoom(accountId, peerId, api, (machine, descriptor, room) => deliverEncryptedRoomEvent(machine, descriptor, room, id, api));
}
export async function discardDirectMatrixText(accountId, peerId, id, api) {
  return inRoom(accountId, peerId, api, (machine, descriptor, room) => discardEncryptedRoomEvent(machine, descriptor, room, id));
}
export async function pinDirectMatrixMessage(accountId, peerId, id, pinned, api) {
  if (!/^[a-f0-9]{24}$/.test(id || "") || typeof pinned !== "boolean") throw new Error("Invalid message.");
  return inRoom(accountId, peerId, api, async (_machine, descriptor, room) => {
    const result = (await api.post("/matrix/messages/events/pin", { roomId: room.roomId, deviceId: descriptor.deviceId, eventId: id, pinned })).data;
    if (result?._id !== id || result.roomId !== room.roomId || result.pinned !== pinned || (pinned ? result.expiresAt !== null : !Number.isFinite(Date.parse(result.expiresAt)))) throw new Error("Pin update was not confirmed.");
    return { pinned, expiresAt: result.expiresAt };
  });
}
export async function deleteDirectMatrixMessage(accountId, peerId, id, api) {
  if (!/^[a-f0-9]{24}$/.test(id || "")) throw new Error("Invalid message.");
  return inRoom(accountId, peerId, api, async (_machine, descriptor, room) => {
    const result = (await api.post("/matrix/messages/events/delete", { roomId: room.roomId, deviceId: descriptor.deviceId, eventId: id })).data;
    if (result?.id !== id) throw new Error("Message deletion was not confirmed.");
    return result;
  });
}
