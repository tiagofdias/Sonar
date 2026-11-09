import { useEffect, useRef, useState } from "react";
import { Search, X } from "lucide-react";

// Looks through a conversation on this device. Nothing typed here is sent anywhere.
export default function SearchPanel({ search, senderName, onClose }) {
  const [query, setQuery] = useState(""), [results, setResults] = useState(null), [busy, setBusy] = useState(false);
  const run = useRef(0);
  useEffect(() => () => { run.current++; }, []);
  const submit = async (event) => {
    event.preventDefault();
    if (!query.trim() || busy) return;
    const token = ++run.current;
    setBusy(true);
    setResults(null);
    try {
      const found = await search(query, () => token === run.current);
      if (token === run.current) setResults(found);
    } finally { if (token === run.current) setBusy(false); }
  };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <section className="detail-modal search-panel" role="dialog" aria-modal="true" aria-label="Search messages" onClick={(event) => event.stopPropagation()}>
        <button className="modal-close icon-button" aria-label="Close" onClick={onClose}><X size={20} /></button>
        <Search className="accent" size={28} />
        <h2>Find it again.</h2>
        <p>Searched on your device. The server never sees what you look for.</p>
        <form className="friend-add" onSubmit={submit}>
          <label className="search-box modal-search">
            <Search size={15} />
            <input autoFocus aria-label="Search messages" placeholder="Search this conversation" maxLength={200} value={query} onChange={(event) => setQuery(event.target.value)} />
          </label>
          <button className="primary compact" disabled={busy || !query.trim()}>{busy ? "Searching..." : "Search"}</button>
        </form>
        <div className="modal-people">
          {results && !results.length && <p>Nothing found in the last 7 days or your pinned messages.</p>}
          {results?.map((message) => (
            <div className="search-result" key={message._id}>
              <small><strong>{senderName(message)}</strong> · {new Date(message.createdAt).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</small>
              <p>{message.text}</p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
