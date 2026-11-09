const MAX_PAGES = 40;

// Runs entirely on this device: reads pages of history, decrypts them and looks through them in memory.
export async function searchHistory(fetchPage, open, query, stillWanted = () => true) {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const found = [];
  let before;
  for (let page = 0; page < MAX_PAGES; page++) {
    const data = await fetchPage(before);
    const opened = await Promise.all(data.messages.map(open));
    if (!stillWanted()) return found;
    found.push(...opened.filter((message) => !message.reaction && !message.unreadable && message.text?.toLowerCase().includes(needle)));
    if (!data.hasMore) break;
    before = data.nextCursor;
  }
  return found;
}
