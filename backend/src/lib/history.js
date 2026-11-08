export function historyQuery(query = {}) {
  const limit = query.limit === undefined ? 50 : Number(query.limit);
  if (
    !Number.isInteger(limit) ||
    limit < 1 ||
    limit > 100 ||
    (query.pinned !== undefined && !["true", "false"].includes(query.pinned))
  )
    throw new Error("Invalid history options");
  let before;
  if (query.before !== undefined) {
    if (
      typeof query.before !== "string" ||
      !/^[A-Za-z0-9_-]{1,256}$/.test(query.before)
    )
      throw new Error("Invalid history cursor");
    const cursor = JSON.parse(
      Buffer.from(query.before, "base64url").toString("utf8"),
    );
    if (
      typeof cursor.at !== "string" ||
      !Number.isFinite(Date.parse(cursor.at)) ||
      new Date(cursor.at).toISOString() !== cursor.at ||
      typeof cursor.id !== "string" ||
      !/^[a-f0-9]{24}$/i.test(cursor.id)
    )
      throw new Error("Invalid history cursor");
    before = {
      $or: [
        { createdAt: { $lt: new Date(cursor.at) } },
        { createdAt: new Date(cursor.at), _id: { $lt: cursor.id } },
      ],
    };
  }
  return {
    limit,
    paged: query.page === "true",
    filters: [
      ...(query.pinned === "true" ? [{ pinned: true }] : []),
      ...(before ? [before] : []),
    ],
  };
}
export function historyPage(rows, limit, transform = (value) => value) {
  const hasMore = rows.length > limit;
  const selected = rows.slice(0, limit);
  const oldest = selected.at(-1);
  const nextCursor =
    hasMore && oldest
      ? Buffer.from(
          JSON.stringify({
            at: new Date(oldest.createdAt).toISOString(),
            id: String(oldest._id),
          }),
        ).toString("base64url")
      : null;
  return { messages: selected.reverse().map(transform), hasMore, nextCursor };
}

// Preserve the existing deadline when an already-unpinned message is updated again.
// The pipeline evaluates the previous state atomically, including concurrent requests.
export function pinUpdate(pinned, week, now = Date.now()) {
  return [{ $set: {
    expiresAt: pinned ? null : { $cond: [{ $eq: ["$pinned", true] }, new Date(now + week), "$expiresAt"] },
    pinned,
  } }];
}
