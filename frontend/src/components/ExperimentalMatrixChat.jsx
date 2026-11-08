import { useEffect, useRef, useState } from "react";
import { axiosInstance as api } from "../lib/axios";
import { REACTION_EMOJI } from "../lib/messageContent.js";

// Deliberately separate from legacy history: this development-only view never
// migrates, replaces, or silently downgrades existing conversations.
export default function ExperimentalMatrixChat({ accountId, peer, socket }) {
  const [messages, setMessages] = useState([]), [pending, setPending] = useState([]), [cursor, setCursor] = useState(null);
  const [text, setText] = useState(""), [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const [removing, setRemoving] = useState(null);
  const [discarding, setDiscarding] = useState(null);
  const [replyTo, setReplyTo] = useState(null);
  const [file, setFile] = useState(null);
  const fileInput = useRef(null);
  const [onlyPins, setOnlyPins] = useState(false);
  const busyRef = useRef(false), dirty = useRef(false), refreshRef = useRef(null);
  const run = async (operation, clearDraftOnPending = false) => {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError("");
    try { await operation(); } catch (failure) {
      if (failure.name === "PendingMatrixMessage" && failure.outboxId) { setPending(items => items.some(item => item.id === failure.outboxId) ? items : [...items, { id: failure.outboxId }]); if (clearDraftOnPending) { setText(""); setReplyTo(null); setFile(null); if (fileInput.current) fileInput.current.value = ""; } }
      setError(failure.response?.data?.message || failure.message || "Matrix operation failed.");
    } finally { busyRef.current = false; setBusy(false); }
  };
  const load = async (older, pins, notify = false) => {
    const { readDirectMatrixHistory } = await import("../lib/matrixDirectClient.js");
    const page = await readDirectMatrixHistory(accountId, peer._id, api, { ...(pins ? { pinned: true } : {}), ...(older && cursor ? { before: cursor } : {}) });
    setMessages(current => older ? [...page.messages, ...current.filter(item => !page.messages.some(previous => previous.id === item.id))] : page.messages);
    setCursor(page.nextCursor); setPending(page.pending);
    if (notify) setNotice("Matrix history refreshed.");
  };
  const refresh = (older, notify = false) => run(() => load(older, onlyPins, notify));
  const togglePins = () => run(async () => { const next = !onlyPins; await load(false, next, false); setOnlyPins(next); });
  refreshRef.current = () => refresh(false, false);
  useEffect(() => {
    const update = () => { if (busyRef.current) dirty.current = true; else refreshRef.current?.(); };
    update();
    socket?.on("matrixRoomChanged", update);
    socket?.on("connect", update);
    return () => { socket?.off("matrixRoomChanged", update); socket?.off("connect", update); };
  }, [socket, accountId, peer._id]);
  useEffect(() => { if (!busy && dirty.current) { dirty.current = false; refreshRef.current?.(); } }, [busy]);
  useEffect(() => {
    const prune = () => setMessages(items => items.filter(item => item.pinned || Date.parse(item.expiresAt) > Date.now()));
    const timer = setInterval(prune, 15000);
    document.addEventListener("visibilitychange", prune);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", prune); };
  }, []);
  const send = event => { event.preventDefault(); run(async () => {
    const { sendDirectMatrixText } = await import("../lib/matrixDirectClient.js");
    await sendDirectMatrixText(accountId, peer._id, text, api, replyTo);
    setText(""); setReplyTo(null); dirty.current = true; setNotice("Encrypted message sent.");
  }, true); };
  const retry = id => run(async () => {
    const { retryDirectMatrixText } = await import("../lib/matrixDirectClient.js");
    const result = await retryDirectMatrixText(accountId, peer._id, id, api);
    setPending(items => items.filter(item => item.id !== id));
    dirty.current = true;
    setNotice(result.unavailable ? "Delivery was previously accepted, but the message is no longer available." : "Pending encrypted message delivered.");
  });
  const discard = id => run(async () => {
    const { discardDirectMatrixText } = await import("../lib/matrixDirectClient.js");
    await discardDirectMatrixText(accountId, peer._id, id, api);
    setPending(items => items.filter(item => item.id !== id)); setDiscarding(null);
    setNotice("Local retry discarded. The message may already have been delivered.");
  });
  const sendFile = () => run(async () => {
    const { sendDirectMatrixAttachment } = await import("../lib/matrixDirectClient.js");
    await sendDirectMatrixAttachment(accountId, peer._id, file, api);
    setFile(null); if (fileInput.current) fileInput.current.value = ""; dirty.current = true; setNotice("Encrypted attachment sent.");
  }, true);
  const download = item => run(async () => {
    const { downloadDirectMatrixAttachment } = await import("../lib/matrixDirectClient.js");
    const bytes = await downloadDirectMatrixAttachment(accountId, peer._id, item.attachment, api);
    const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" })); bytes.fill(0);
    const link = document.createElement("a"); link.href = url; link.download = item.attachment.name; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 60000);
  });
  const pin = item => run(async () => {
    const { pinDirectMatrixMessage } = await import("../lib/matrixDirectClient.js");
    const result = await pinDirectMatrixMessage(accountId, peer._id, item.id, !item.pinned, api);
    setMessages(items => onlyPins && !result.pinned ? items.filter(message => message.id !== item.id) : items.map(message => message.id === item.id ? { ...message, ...result } : message));
    setNotice(result.pinned ? "Message pinned." : "Message unpinned; its seven-day period has restarted.");
  });
  const remove = () => run(async () => {
    const { deleteDirectMatrixMessage } = await import("../lib/matrixDirectClient.js");
    await deleteDirectMatrixMessage(accountId, peer._id, removing, api);
    setMessages(items => items.filter(item => item.id !== removing)); setRemoving(null); setNotice("Message deleted.");
  });
  const react = (item, emoji, ownReaction) => run(async () => {
    if (ownReaction) {
      const { deleteDirectMatrixMessage } = await import("../lib/matrixDirectClient.js");
      await deleteDirectMatrixMessage(accountId, peer._id, ownReaction.id, api);
    } else {
      const { sendDirectMatrixReaction } = await import("../lib/matrixDirectClient.js");
      await sendDirectMatrixReaction(accountId, peer._id, item.id, emoji, api);
    }
    dirty.current = true; setNotice("Reaction updated.");
  });
  const contentMessages = messages.filter(item => !item.reaction);
  const reactions = messages.filter(item => item.reaction && !item.unreadable);
  return <section className="settings-retention" data-matrix-chat aria-busy={busy}>
    <h3>Experimental Matrix messages</h3>
    <p>This test view is separate from your existing conversation. Both people must prepare and verify their Matrix devices. Messages update while connected.</p>
    <button type="button" className="secondary" disabled={busy} onClick={() => refresh(false, true)}>Refresh Matrix history</button>
    <button type="button" className="secondary" disabled={busy} onClick={togglePins}>{onlyPins ? "Show all Matrix messages" : "Show pinned Matrix messages"}</button>
    {cursor && <button type="button" className="secondary" disabled={busy} onClick={() => refresh(true, true)}>Load older Matrix messages</button>}
    <ol aria-label="Matrix messages">{contentMessages.map(item => <li key={item.id} data-matrix-message-id={item.id}>
      {item.replyTo && <p>Reply to: {contentMessages.find(message => message.id === item.replyTo)?.text || "Unavailable message"}</p>}
      <strong>{item.sender.startsWith(`@${accountId}:`) ? "You" : peer.fullName}</strong>: {item.unreadable ? "Unable to verify or decrypt this message." : item.attachment ? <button type="button" className="secondary" disabled={busy} onClick={() => download(item)}>Download {item.attachment.name} ({item.attachment.size} bytes)</button> : item.text} {item.pinned && <span>(Pinned)</span>}
      {!item.unreadable && <><button type="button" className="secondary" disabled={busy || pending.length > 0} onClick={() => setReplyTo(item.id)}>Reply to Matrix message</button>{!onlyPins && REACTION_EMOJI.map(emoji => {
        const matching = reactions.filter(event => event.reaction.target === item.id && event.reaction.emoji === emoji);
        const own = matching.find(event => event.sender.startsWith(`@${accountId}:`));
        return <button type="button" className="secondary" key={emoji} aria-label={`${own ? "Remove" : "Add"} ${emoji} Matrix reaction`} aria-pressed={Boolean(own)} disabled={busy || pending.length > 0} onClick={() => react(item, emoji, own)}>{emoji}{matching.length ? ` ${matching.length}` : ""}</button>;
      })}</>}
      <button type="button" className="secondary" disabled={busy} onClick={() => pin(item)}>{item.pinned ? "Unpin Matrix message" : "Pin Matrix message"}</button> {item.sender.startsWith(`@${accountId}:`) && <button type="button" className="secondary" disabled={busy} onClick={() => setRemoving(item.id)}>Delete Matrix message</button>}</li>)}</ol>
    {removing && <div role="alert"><p>Delete this encrypted message for both participants? Copies already saved elsewhere cannot be removed.</p><button type="button" className="secondary" disabled={busy} onClick={remove}>Confirm Matrix deletion</button><button type="button" className="secondary" disabled={busy} onClick={() => setRemoving(null)}>Keep Matrix message</button></div>}
    {pending.map(item => <div key={item.id}><p>A message is awaiting confirmation. <button type="button" className="secondary" disabled={busy} onClick={() => retry(item.id)}>Retry pending message</button> <button type="button" className="secondary" disabled={busy} onClick={() => setDiscarding(item.id)}>Discard pending retry</button></p>{discarding === item.id && <div role="alert"><p>Discard this local retry? The message may already have been delivered.</p><button type="button" className="secondary" disabled={busy} onClick={() => discard(item.id)}>Confirm discard</button><button type="button" className="secondary" disabled={busy} onClick={() => setDiscarding(null)}>Keep retry</button></div>}</div>)}
    {replyTo && <p>Replying to: {contentMessages.find(item => item.id === replyTo)?.text || "Unavailable message"} <button type="button" className="secondary" onClick={() => setReplyTo(null)}>Cancel Matrix reply</button></p>}
    <form onSubmit={send}><label className="dialog-field">Matrix message<textarea value={text} onChange={event => setText(event.target.value)} maxLength={32000} disabled={busy || pending.length > 0} /></label><button type="submit" className="secondary" disabled={busy || !text.trim() || pending.length > 0}>Send Matrix message</button></form>
    <label className="dialog-field">Encrypted attachment (up to 10 MiB)<input ref={fileInput} type="file" disabled={busy || pending.length > 0} onChange={event => setFile(event.target.files?.[0] || null)} /></label><button type="button" className="secondary" disabled={busy || !file || pending.length > 0} onClick={sendFile}>Send encrypted attachment</button>
    {notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
  </section>;
}
