// Finds a community invite link inside a chat message. `only` is true when the
// message is nothing but the link, so the card can replace it completely.
export function findInvite(text) {
  if (typeof text !== "string") return null;
  const match = text.match(/[?&]invite=([a-f0-9]{48})(?![a-f0-9])/i);
  if (!match) return null;
  return { token: match[1].toLowerCase(), only: !/\s/.test(text.trim()) };
}
