import { useEffect, useRef, useState } from "react";
import { LayoutGrid, MonitorPlay, Tv } from "lucide-react";

// Apps that run inside a call or voice room. Watch Together is the first.
const APPS = [
  { id: "watch", name: "Watch together", note: "YouTube in sync, with sponsor segments skipped", icon: MonitorPlay },
  // Sharing a TV channel starts from the desktop app (browsers can't open IPTV streams); web members watch it here.
  { id: "tv", name: "Sonar TV", note: "Share your IPTV channel from the Sonar desktop app. Click to download it.", icon: Tv },
];

export default function AppsMenu({ watch }) {
  const [open, setOpen] = useState(false);
  const box = useRef(null);
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
    if (id === "tv") window.open("/downloads/Sonar-Desktop-windows-x64.zip", "_blank", "noopener");
  };
  return (
    <span className="apps-menu" ref={box}>
      {watch.available && !watch.joined && (
        <button className="secondary compact apps-join" onClick={watch.join} title={watch.available.title || "Watch together"}>
          <MonitorPlay size={15} /> Join watch together{watch.available.viewers ? ` · ${watch.available.viewers}` : ""}
        </button>
      )}
      <button className={`icon-button ${watch.joined ? "accent" : ""}`} aria-label="Apps" title="Apps" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <LayoutGrid size={18} />
      </button>
      {open && (
        <span className="apps-options" role="menu" aria-label="Apps">
          {APPS.map(({ id, name, note, icon: Icon }) => (
            <button key={id} role="menuitem" onClick={() => start(id)} disabled={id === "watch" && watch.joined}>
              <Icon size={18} />
              <span><strong>{name}</strong><small>{id === "watch" && watch.joined ? "You're watching" : note}</small></span>
            </button>
          ))}
        </span>
      )}
    </span>
  );
}
