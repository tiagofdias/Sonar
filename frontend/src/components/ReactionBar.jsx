import { useState } from "react";
import { SmilePlus } from "lucide-react";
import { REACTION_EMOJI } from "../lib/messageContent";
export default function ReactionBar({ message, reactions = {}, me, onReact, disabled }) {
  const [open, setOpen] = useState(false), [busy, setBusy] = useState(false);
  const shown = open ? REACTION_EMOJI : REACTION_EMOJI.filter(emoji => reactions[emoji]?.length);
  return <div className="message-reactions">
    {shown.map(emoji => {
      const users = reactions[emoji] || [], selected = users.includes(me);
      return <button key={emoji} className={`reaction-button ${selected ? "selected" : ""}`}
        aria-label={`${selected ? "Remove" : "Add"} ${emoji} reaction`} aria-pressed={selected}
        disabled={disabled || busy} onClick={async () => {
          setBusy(true);
          try { await onReact({ target: message._id, emoji, action: selected ? "remove" : "add" }); }
          finally { setBusy(false); }
        }}>{emoji}{users.length > 0 && <span>{users.length}</span>}</button>;
    })}
    <button className="reaction-picker" title="Add reaction" aria-label="Add reaction" aria-expanded={open} disabled={disabled} onClick={() => setOpen(!open)}><SmilePlus size={14} /></button>
  </div>;
}
