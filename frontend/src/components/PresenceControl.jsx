import { useState } from "react";
import { useAuthStore } from "../store/useAuthStore";
import { PRESENCE_OPTIONS, presenceLabel } from "../lib/presence";
import { initials } from "../lib/initials";

// The account row at the bottom of the sidebar, where you choose your status.
export default function PresenceControl({ children }) {
  const { authUser, setPresence } = useAuthStore();
  const [open, setOpen] = useState(false);
  const current = authUser.presence || "online";
  return (
    <>
      {open && (
        <div className="status-menu" role="menu" aria-label="Set your status">
          {PRESENCE_OPTIONS.map(([value, label]) => (
            <button key={value} role="menuitem" className={value === current ? "selected" : ""} onClick={() => { setOpen(false); setPresence(value); }}>
              <i className={`status-dot status-${value}`} />
              {label}
            </button>
          ))}
        </div>
      )}
      <footer className="user-footer">
        <button className="status-trigger" title="Set your status" aria-expanded={open} onClick={() => setOpen(!open)}>
          <span className="avatar-tile">
            {initials(authUser.fullName)}
            <i className={`status-${current}`} />
          </span>
          <span className="status-text">
            <strong>{authUser.fullName}</strong>
            <small>{presenceLabel(current)}</small>
          </span>
        </button>
        {children}
      </footer>
    </>
  );
}
