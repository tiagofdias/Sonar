import { useEffect, useRef, useState } from "react";
import toast from "react-hot-toast";
import { LayoutGrid, MonitorPlay, Music, Tv } from "lucide-react";

// Apps that run inside a call or voice room. Same list and look as the desktop app.
const APPS = [
  { id: "watch", name: "Watch together", note: "YouTube in sync, with sponsor segments skipped", icon: MonitorPlay },
  // Sharing a TV channel starts from the desktop app (browsers can't open IPTV streams); web members watch it here.
  { id: "music", name: "Listen together", note: "Music from someone's PC, heard by everyone in high quality. Playing it starts in the desktop app (listening works here). Click to get it.", icon: Music },
  { id: "tv", name: "Sonar TV", note: "Browsers can't open IPTV streams, so sharing starts in the desktop app (watching works here). Click to get it.", icon: Tv },
];

// Says once when someone starts sharing TV, with a Join button; nothing opens until you choose.
function useTvNotices(screen, nameOf) {
  const seen = useRef(new Set());
  const available = screen?.available || {};
  const key = Object.keys(available).join(",");
  useEffect(() => {
    const ids = key ? key.split(",") : [];
    for (const id of [...seen.current]) if (!ids.includes(id)) { seen.current.delete(id); toast.dismiss(`tv-${id}`); }
    for (const id of ids) {
      if (seen.current.has(id)) continue;
      seen.current.add(id);
      toast((t) => (
        <span className="tv-notice">
          <Tv size={18} />
          <span><strong>{nameOf(id)} is sharing TV</strong><small>{available[id]}</small></span>
          <button className="primary compact" onClick={() => { toast.dismiss(t.id); screen.join(id); }}>Join</button>
        </span>
      ), { id: `tv-${id}`, duration: 12000 });
    }
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
}

export default function AppsMenu({ watch, screen, nameOf = () => "Someone" }) {
  const [open, setOpen] = useState(false);
  const box = useRef(null);
  useTvNotices(screen, nameOf);
  useEffect(() => {
    if (!open) return undefined;
    const outside = (event) => { if (!box.current?.contains(event.target)) setOpen(false); };
    const escape = (event) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("mousedown", outside); document.removeEventListener("keydown", escape); };
  }, [open]);
  if (!watch) return null;
  const start = (id) => {
    setOpen(false);
    if (id === "watch" && !watch.joined) watch.join();
    if (id === "tv" || id === "music") window.open("/downloads/Sonar-Desktop-windows-x64.zip", "_blank", "noopener");
  };
  const tvShares = Object.entries(screen?.available || {});
  return (
    <span className="apps-menu" ref={box}>
      {tvShares.map(([id, label]) => (
        <button key={id} className="secondary compact apps-join" onClick={() => screen.join(id)} title={`${nameOf(id)} is sharing TV: ${label}`}>
          <Tv size={15} /> Join TV · {label}
        </button>
      ))}
      {watch.available && !watch.joined && (
        <button className="secondary compact apps-join" onClick={watch.join} title={watch.available.title || "Watch together"}>
          <MonitorPlay size={15} /> Join watch together{watch.available.viewers ? ` · ${watch.available.viewers}` : ""}
        </button>
      )}
      <button className={`icon-button ${open || watch.joined ? "accent" : ""}`} aria-label="Apps" title="Apps" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <LayoutGrid size={18} />
      </button>
      {open && (
        <span className="apps-options" role="menu" aria-label="Apps">
          <span className="apps-title">Apps</span>
          {APPS.map(({ id, name, note, icon: Icon }) => (
            <button key={id} role="menuitem" onClick={() => start(id)} disabled={id === "watch" && watch.joined}>
              <span className="apps-icon"><Icon size={18} /></span>
              <span><strong>{name}</strong><small>{id === "watch" && watch.joined ? "You're watching" : note}</small></span>
            </button>
          ))}
        </span>
      )}
    </span>
  );
}
