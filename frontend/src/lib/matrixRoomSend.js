// Experimental direct message composition. Callers hold the protected SDK
// store lock and provide a freshly checked room snapshot. This is not wired to
// the chat UI until cross-account verification and recovery are available.
import { RoomId } from "@matrix-org/matrix-sdk-crypto-wasm";
import { discardStaleRoomKeyShares, pendingRoomKeyShareState, prepareTrustedRoomKeyShares, deliverTrustedRoomKeyShares } from "./matrixShareOutbox.js";
import { queueEncryptedRoomEvent, deliverEncryptedRoomEvent, pendingEncryptedRoomEvents } from "./matrixEventOutbox.js";
import { REACTION_EMOJI } from "./messageContent.js";
import { encryptMatrixAttachment, MAX_MATRIX_ATTACHMENT_BYTES } from "./matrixAttachment.js";

export class PendingMatrixMessage extends Error {
  constructor(id, cause) { super("Message delivery is uncertain. Retry the existing encrypted message.", { cause }); this.name = "PendingMatrixMessage"; this.outboxId = id; }
}
export async function sendTrustedRoomText(machine, descriptor, room, text, api, replyTo = null) {
  if (typeof text !== "string" || !text.trim() || text.length > 32000 || new TextEncoder().encode(text).length > 32000) throw new Error("Invalid message text.");
  if (replyTo !== null && (typeof replyTo !== "string" || !/^[a-f0-9]{24}$/.test(replyTo))) throw new Error("Invalid reply target.");
  return sendTrustedRoomEvent(machine, descriptor, room, "m.room.message", { msgtype: "m.text", body: text, ...(replyTo ? { "m.relates_to": { "m.in_reply_to": { event_id: `$${replyTo}` } } } : {}) }, api);
}
export async function sendTrustedRoomReaction(machine, descriptor, room, target, emoji, api) {
  if (typeof target !== "string" || !/^[a-f0-9]{24}$/.test(target) || !REACTION_EMOJI.includes(emoji)) throw new Error("Invalid reaction.");
  return sendTrustedRoomEvent(machine, descriptor, room, "m.reaction", { "m.relates_to": { rel_type: "m.annotation", event_id: `$${target}`, key: emoji } }, api);
}
export async function sendTrustedRoomAttachment(machine, descriptor, room, file, api) {
  // eslint-disable-next-line no-control-regex
  if (!(file instanceof Blob) || typeof file.name !== "string" || !file.name || file.name.length > 255 || /[\x00-\x1f\x7f/\\]/.test(file.name) || file.size < 1 || file.size > MAX_MATRIX_ATTACHMENT_BYTES) throw new Error("Invalid attachment.");
  const domain = /^@[a-f0-9]{24}:([a-z0-9.-]{1,200})$/.exec(descriptor.userId || "")?.[1];
  if (!domain) throw new Error("Invalid device identity.");
  const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), value => value.toString(16).padStart(2, "0")).join("");
  const plaintext = new Uint8Array(await file.arrayBuffer());
  let encrypted;
  try { encrypted = encryptMatrixAttachment(plaintext); } finally { plaintext.fill(0); }
  const metadata = JSON.parse(encrypted.metadata);
  const event = { msgtype: "m.file", body: file.name, info: { size: file.size, mimetype: "application/octet-stream" }, file: { ...metadata, url: `mxc://${domain}/${id}` } };
  return sendTrustedRoomEvent(machine, descriptor, room, "m.room.message", event, api, { id, ciphertext: encrypted.ciphertext });
}
async function sendTrustedRoomEvent(machine, descriptor, room, type, event, api, attachment = null) {
  if ((await pendingEncryptedRoomEvents(descriptor)).some(item => item.roomId === room?.roomId)) throw new Error("Retry the pending message before sending another.");
  await discardStaleRoomKeyShares(machine, descriptor, room, api);
  let state = await pendingRoomKeyShareState(descriptor, room);
  if (state.preparing) await prepareTrustedRoomKeyShares(machine, descriptor, room, api);
  state = await pendingRoomKeyShareState(descriptor, room);
  if (state.count) await deliverTrustedRoomKeyShares(machine, descriptor, room, api);
  await prepareTrustedRoomKeyShares(machine, descriptor, room, api);
  await deliverTrustedRoomKeyShares(machine, descriptor, room, api);
  const id = new RoomId(room.roomId);
  let content;
  try { content = JSON.parse(await machine.encryptRoomEvent(id, type, JSON.stringify(event))); }
  finally { id.free(); }
  // The SDK may copy relations outside the ciphertext for Matrix servers to
  // index. Sonar keeps those references inside the encrypted event instead.
  delete content["m.relates_to"];
  const outboxId = await queueEncryptedRoomEvent(machine, descriptor, room, content, api, attachment);
  try { return await deliverEncryptedRoomEvent(machine, descriptor, room, outboxId, api); }
  catch (error) { throw new PendingMatrixMessage(outboxId, error); }
}
