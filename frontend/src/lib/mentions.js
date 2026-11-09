export const MAX_MENTIONS = 32;
const lower = (value) => value.toLowerCase();

// Who a finished message names. The list travels inside the encrypted message.
export function mentionsIn(text, members) {
  const value = lower(text);
  const found = members.filter((member) => value.includes(`@${lower(member.fullName)}`)).map((member) => member._id);
  if (/(^|\s)@everyone\b/i.test(text)) found.push("everyone");
  return found.slice(0, MAX_MENTIONS);
}

// The name being typed right after an @, if any.
export function mentionQuery(text) {
  return /(?:^|\s)@([^\s@]*)$/.exec(text)?.[1] ?? null;
}

export function completeMention(text, name) {
  return text.replace(/@[^\s@]*$/, `@${name} `);
}

export function mentionsMe(decoded, me, senderCanEveryone) {
  const list = decoded?.mentions || [];
  return list.includes(me) || (senderCanEveryone && list.includes("everyone"));
}
