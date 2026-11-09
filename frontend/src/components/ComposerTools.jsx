import { useRef, useState } from "react";
import { Download, File as FileIcon, Paperclip, Smile } from "lucide-react";
import toast from "react-hot-toast";
import { formatBytes, MAX_FILE_BYTES, saveFile, uploadFile } from "../lib/attachments";
import { EMOJI } from "../lib/emoji";

const message = (error) => error.response?.data?.message || "Could not send the file";

// Emoji and attachment buttons for a message box. `send` receives the uploaded
// file's details, and `stillHere` guards against the conversation having changed.
export default function ComposerTools({ textarea, value, setValue, target, send, stillHere, disabled }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef(null);
  const addEmoji = (emoji) => {
    const box = textarea.current;
    const [from, to] = box ? [box.selectionStart, box.selectionEnd] : [value.length, value.length];
    setValue(value.slice(0, from) + emoji + value.slice(to));
    requestAnimationFrame(() => { box?.focus(); box?.setSelectionRange(from + emoji.length, from + emoji.length); });
  };
  const choose = async (event) => {
    const file = event.target.files[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > MAX_FILE_BYTES) return toast.error("Files can be up to 8 MB");
    if (file.size === 0) return toast.error("That file is empty");
    setBusy(true);
    try {
      const meta = await uploadFile(file, target);
      if (stillHere()) await send(meta);
    } catch (error) {
      toast.error(message(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <span className="composer-tools">
      {open && (
        <span className="emoji-grid" role="menu" aria-label="Emoji">
          {EMOJI.map((emoji) => <button key={emoji} type="button" role="menuitem" onClick={() => addEmoji(emoji)}>{emoji}</button>)}
        </span>
      )}
      <button type="button" className="composer-tool" title="Emoji" aria-label="Emoji" aria-expanded={open} disabled={disabled} onClick={() => setOpen(!open)}><Smile size={18} /></button>
      <button type="button" className="composer-tool" title={busy ? "Encrypting and sending..." : "Attach a file (up to 8 MB)"} aria-label="Attach a file" disabled={disabled || busy} onClick={() => input.current.click()}><Paperclip size={18} /></button>
      <input ref={input} type="file" hidden aria-label="Choose a file" onChange={choose} />
    </span>
  );
}

export function FileCard({ file }) {
  const [busy, setBusy] = useState(false);
  const download = async () => {
    setBusy(true);
    try { await saveFile(file); } catch (error) { toast.error(error.response ? "This file is no longer available" : "Could not open this file"); } finally { setBusy(false); }
  };
  return (
    <div className="file-card">
      <span className="invite-card-icon"><FileIcon size={19} /></span>
      <span className="invite-card-text"><strong>{file.name}</strong><small>{formatBytes(file.size)} · Encrypted</small></span>
      <button className="invite-open" disabled={busy} aria-label={`Download ${file.name}`} onClick={download}><Download size={16} /></button>
    </div>
  );
}
