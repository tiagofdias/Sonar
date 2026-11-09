const PREFIX = "sonar-reply-v1:";
const REACTION_PREFIX = "sonar-reaction-v1:";
const CALL_PREFIX = "sonar-call-v1:";
const FILE_PREFIX = "sonar-file-v1:";
export const MISSED_CALL_TEXT = "Missed voice call";
export const REACTION_EMOJI = ["\u{1F44D}", "\u2764\uFE0F", "\u{1F602}", "\u{1F389}"];
const messageId = /^[a-f0-9]{24}$/i;
// Only the reply target ID is carried inside encrypted content. No copied quote is stored.
const validMentions = (list) => Array.isArray(list) && list.length <= 32 && list.every((item) => item === "everyone" || (typeof item === "string" && messageId.test(item)));
export function encodeMessage(text, replyTo = null, mentions = []) {
  if (replyTo && !messageId.test(replyTo)) throw new Error("Invalid reply target");
  const named = mentions.length && validMentions(mentions) ? { mentions } : {};
  if (named.mentions || replyTo || text.startsWith(PREFIX) || text.startsWith(REACTION_PREFIX) || text.startsWith(CALL_PREFIX) || text.startsWith(FILE_PREFIX))
    return PREFIX + JSON.stringify({ text, ...(replyTo ? { replyTo } : {}), ...named });
  return text;
}
// A call that rang and was never answered, sent by the caller's app.
export function encodeMissedCall() {
  return CALL_PREFIX + JSON.stringify({ event: "missed" });
}
const validFile = (file) =>
  /^[a-f0-9]{24}$/i.test(file?.id) && /^[A-Za-z0-9+/]{43}=$/.test(file.key) && /^[A-Za-z0-9+/]{16}$/.test(file.iv) &&
  typeof file.name === "string" && file.name.length > 0 && file.name.length <= 255 &&
  typeof file.type === "string" && file.type.length <= 120 &&
  Number.isInteger(file.size) && file.size > 0 && file.size <= 8388608;
export const fileLabel = (file) => `📎 ${file.name}`;
export function encodeFile(file) {
  if (!validFile(file)) throw new Error("Invalid file");
  return FILE_PREFIX + JSON.stringify({ id: file.id, key: file.key, iv: file.iv, name: file.name, type: file.type, size: file.size });
}
export function encodeReaction(reaction) {
  if (!validReaction(reaction)) throw new Error("Invalid reaction");
  return REACTION_PREFIX + JSON.stringify(reaction);
}
function validReaction(reaction) {
  return typeof reaction?.target === "string" && messageId.test(reaction.target) &&
    REACTION_EMOJI.includes(reaction.emoji) && ["add", "remove"].includes(reaction.action);
}
export function decodeMessage(value) {
  if (value.startsWith(FILE_PREFIX)) {
    try {
      const file = JSON.parse(value.slice(FILE_PREFIX.length));
      if (validFile(file)) return { text: fileLabel(file), file };
    } catch { /* Preserve invalid content as text. */ }
    return { text: value };
  }
  if (value.startsWith(CALL_PREFIX)) {
    try {
      if (JSON.parse(value.slice(CALL_PREFIX.length))?.event === "missed") return { text: MISSED_CALL_TEXT, call: "missed" };
    } catch { /* Preserve invalid content as text. */ }
    return { text: value };
  }
  if (value.startsWith(REACTION_PREFIX)) {
    try {
      const reaction = JSON.parse(value.slice(REACTION_PREFIX.length));
      if (validReaction(reaction)) return { text: "", reaction: { target: reaction.target, emoji: reaction.emoji, action: reaction.action } };
    } catch { /* Preserve invalid or legacy content. */ }
    return { text: value };
  }
  if (!value.startsWith(PREFIX)) return { text: value };
  try {
    const data = JSON.parse(value.slice(PREFIX.length));
    if (typeof data?.text === "string" && data.text.length <= 4000 &&
        (data.replyTo === undefined || (typeof data.replyTo === "string" && messageId.test(data.replyTo))))
      return { text: data.text, ...(data.replyTo ? { replyTo: data.replyTo } : {}), ...(validMentions(data.mentions) && data.mentions.length ? { mentions: data.mentions } : {}) };
  } catch { /* Treat legacy or malformed content as text. */ }
  return { text: value };
}
