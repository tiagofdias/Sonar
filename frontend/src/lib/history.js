export function mergeHistory(...groups) {
  return [
    ...new Map(groups.flat().map((message) => [message._id, message])).values(),
  ].sort(
    (a, b) =>
      new Date(a.createdAt) - new Date(b.createdAt) ||
      a._id.localeCompare(b._id),
  );
}

// The server remains authoritative; this also drops expired plaintext from
// active UI state when a page stays open past a message's deadline.
export function withoutExpired(messages, now = Date.now()) {
  return messages.filter((message) =>
    message.pinned === true || Date.parse(message.expiresAt) > now,
  );
}
