import { REACTION_EMOJI } from "./messageContent.js";
// Events are reduced in the same stable order as message history.
export function reactionSummary(messages, now = Date.now()) {
  const choices = new Map();
  for (const message of messages) {
    const reaction = message.reaction;
    if (!reaction || (!message.pinned && new Date(message.expiresAt).getTime() <= now)) continue;
    choices.set(`${reaction.target}:${message.senderId}:${reaction.emoji}`, {
      ...reaction, senderId: message.senderId,
    });
  }
  const result = {};
  for (const item of choices.values()) {
    if (item.action !== "add" || !REACTION_EMOJI.includes(item.emoji)) continue;
    const target = result[item.target] ||= {};
    (target[item.emoji] ||= []).push(item.senderId);
  }
  return result;
}
