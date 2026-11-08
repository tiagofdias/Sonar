// Experimental Matrix history reader. The caller holds the SDK store lock and
// passes a freshly verified room snapshot. Untrusted or malformed events are
// represented as unreadable, never as plaintext from an unknown identity.
import { RoomId, DecryptionSettings, TrustRequirement } from "@matrix-org/matrix-sdk-crypto-wasm";
import { requireTrustedRoomDevices } from "./matrixRoomKeys.js";
import { REACTION_EMOJI } from "./messageContent.js";

const ID = /^[a-f0-9]{24}$/;
export async function readTrustedRoomHistory(machine, descriptor, room, api, options = {}) {
  if (typeof api?.get !== "function" || typeof api?.post !== "function" || !options || typeof options !== "object" || (options.before !== undefined && (typeof options.before !== "string" || !/^[A-Za-z0-9_-]{1,256}$/.test(options.before))) || (options.limit !== undefined && (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 100))) throw new Error("Invalid history request.");
  const trusted = await requireTrustedRoomDevices(machine, descriptor, room, api);
  const result = (await api.get("/matrix/messages/events/history", { params: { deviceId: descriptor.deviceId, roomId: room.roomId, ...(options.before ? { before: options.before } : {}), ...(options.limit ? { limit: options.limit } : {}), ...(options.pinned ? { pinned: "true" } : {}) } })).data;
  if (!result || !Array.isArray(result.messages) || result.messages.length > 100 || typeof result.hasMore !== "boolean" || !(result.nextCursor === null || (typeof result.nextCursor === "string" && /^[A-Za-z0-9_-]{1,256}$/.test(result.nextCursor)))) throw new Error("Invalid encrypted history.");
  const messages = [];
  for (const row of result.messages) {
    if (typeof row?._id !== "string" || !ID.test(row._id) || row.roomId !== room.roomId || !room.members.includes(row.sender) || typeof row.senderDevice !== "string" || !row.content || typeof row.content !== "object" || typeof row.pinned !== "boolean" || !Number.isFinite(Date.parse(row.createdAt)) || (row.pinned ? row.expiresAt !== null : !Number.isFinite(Date.parse(row.expiresAt)))) throw new Error("Invalid encrypted history event.");
    if (!row.pinned && Date.parse(row.expiresAt) <= Date.now()) continue;
    const fallback = { id: row._id, sender: row.sender, createdAt: row.createdAt, pinned: row.pinned, expiresAt: row.expiresAt, unreadable: true };
    const expected = trusted.find(device => device.userId === row.sender && device.deviceId === row.senderDevice);
    if (!expected) { messages.push(fallback); continue; }
    const roomId = new RoomId(room.roomId), settings = new DecryptionSettings(TrustRequirement.Untrusted);
    let decrypted, sender, device, forwarder, forwarderDevice;
    try {
      const event = { type: "m.room.encrypted", event_id: `$${row._id}`, sender: row.sender, origin_server_ts: Date.parse(row.createdAt), content: row.content };
      decrypted = await machine.decryptRoomEvent(JSON.stringify(event), roomId, settings);
      sender = decrypted.sender; device = decrypted.senderDevice;
      forwarder = decrypted.forwarder; forwarderDevice = decrypted.forwarderDevice;
      const decoded = JSON.parse(decrypted.event);
      if (sender?.toString() !== row.sender || device?.toString() !== row.senderDevice || decrypted.senderCurve25519Key !== expected.curve25519 || decrypted.senderClaimedEd25519Key !== expected.ed25519 || forwarder || forwarderDevice) throw new Error("Untrusted message.");
      const relation = decoded?.content?.["m.relates_to"];
      if (decoded?.type === "m.room.message" && decoded?.content?.msgtype === "m.text" && typeof decoded.content.body === "string" && decoded.content.body.length <= 32000) {
        const replyEvent = relation?.["m.in_reply_to"]?.event_id;
        if (replyEvent !== undefined && (typeof replyEvent !== "string" || !/^\$[a-f0-9]{24}$/.test(replyEvent))) throw new Error("Invalid reply.");
        messages.push({ ...fallback, unreadable: false, text: decoded.content.body, ...(replyEvent ? { replyTo: replyEvent.slice(1) } : {}) });
      } else if (decoded?.type === "m.room.message" && decoded?.content?.msgtype === "m.file") {
        const { body, file, info } = decoded.content;
        const domain = /^@[a-f0-9]{24}:([a-z0-9.-]{1,200})$/.exec(descriptor.userId || "")?.[1];
        const url = typeof file?.url === "string" ? /^mxc:\/\/([a-z0-9.-]{1,200})\/([a-f0-9]{32})$/.exec(file.url) : null;
        // eslint-disable-next-line no-control-regex
        if (!domain || !url || url[1] !== domain || typeof body !== "string" || !body || body.length > 255 || /[\x00-\x1f\x7f/\\]/.test(body) || !Number.isSafeInteger(info?.size) || info.size < 1 || info.size > 10 * 1024 * 1024 || file.v !== "v2") throw new Error("Invalid attachment.");
        const metadata = { ...file };
        delete metadata.url;
        const encoded = JSON.stringify(metadata);
        if (encoded.length > 4096) throw new Error("Invalid attachment.");
        messages.push({ ...fallback, unreadable: false, attachment: { id: url[2], name: body, size: info.size, metadata: encoded } });
      } else if (decoded?.type === "m.reaction" && relation?.rel_type === "m.annotation" && typeof relation.event_id === "string" && /^\$[a-f0-9]{24}$/.test(relation.event_id) && REACTION_EMOJI.includes(relation.key)) {
        messages.push({ ...fallback, unreadable: false, reaction: { target: relation.event_id.slice(1), emoji: relation.key } });
      } else throw new Error("Unsupported message.");
    } catch { messages.push(fallback); }
    finally { sender?.free(); device?.free(); forwarder?.free(); forwarderDevice?.free(); decrypted?.free(); settings.free(); roomId.free(); }
  }
  return { messages, hasMore: result.hasMore, nextCursor: result.nextCursor };
}
