import { useEffect, useState } from "react";
import { ArrowUpRight, Users } from "lucide-react";
import { axiosInstance as api } from "../lib/axios";
import { findInvite } from "../lib/invites";
import { useCommunityStore } from "../store/useCommunityStore";

function InviteCard({ token, onOpen }) {
  const memberships = useCommunityStore((state) => state.communities.length);
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let current = true;
    api.post("/communities/invite-preview", { token })
      .then(({ data }) => current && setPreview(data))
      .catch(() => current && setPreview({ failed: true }));
    return () => { current = false; };
  }, [token, memberships]);
  const store = useCommunityStore.getState;
  const join = async () => {
    setBusy(true);
    try { if (await store().join(token)) onOpen?.(); } finally { setBusy(false); }
  };
  const open = async () => {
    setBusy(true);
    try { await store().select(preview.id); onOpen?.(); } finally { setBusy(false); }
  };
  let title = "Checking invite...", detail = "Community invite", action = null;
  if (preview?.failed) {
    title = "Could not load this invite";
    detail = "Check your connection and reopen the conversation.";
  } else if (preview && !preview.available) {
    title = "Invite expired";
    detail = "Ask for a new invite link.";
  } else if (preview) {
    const members = `${preview.memberCount} ${preview.memberCount === 1 ? "member" : "members"}`;
    title = preview.name;
    detail = preview.member ? `${members} · You're a member` : preview.full ? `${members} · Community full` : `${members} · Invite only`;
    if (preview.member) action = <button className="invite-open" disabled={busy} onClick={open}>Open</button>;
    else if (!preview.full) action = <button className="invite-join" disabled={busy} onClick={join}>{busy ? "Joining..." : "Join"} <ArrowUpRight size={16} /></button>;
  }
  return (
    <div className="invite-card">
      <span className="invite-card-icon"><Users size={19} /></span>
      <span className="invite-card-text"><strong>{title}</strong><small>{detail}</small></span>
      {action}
    </div>
  );
}

const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
function highlighted(text, names) {
  if (!text.includes("@") || !names.length) return text;
  const pattern = new RegExp(`(@(?:everyone|${names.map(escape).join("|")}))`, "gi");
  return text.split(pattern).map((part, index) => (index % 2 ? <span className="mention" key={index}>{part}</span> : part));
}

// A message's text, with community invite links shown as a joinable card.
export default function MessageText({ text, onOpenCommunity, names = [] }) {
  const invite = findInvite(text);
  return (
    <>
      {!invite?.only && <p>{highlighted(text, names)}</p>}
      {invite && <InviteCard token={invite.token} onOpen={onOpenCommunity} />}
    </>
  );
}
