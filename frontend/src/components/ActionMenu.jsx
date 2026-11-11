import { useEffect, useRef, useState } from "react";
import { MoreHorizontal } from "lucide-react";

// A "⋯" button that opens a short list of labelled actions, such as those for one person.
// Items are { label, icon, danger, onClick }, "-" for a divider, or false to leave one out.
// It floats above the page, so a narrow or scrolling panel never cuts it off.
export default function ActionMenu({ label, items }) {
  const [at, setAt] = useState(null);
  const button = useRef(null);
  const menu = useRef(null);
  const shown = items.filter(Boolean).filter((item, index, list) => item !== "-" || (index > 0 && index < list.length - 1 && list[index - 1] !== "-"));
  useEffect(() => {
    if (!at) return undefined;
    const outside = (event) => { if (!menu.current?.contains(event.target) && !button.current?.contains(event.target)) setAt(null); };
    const escape = (event) => { if (event.key === "Escape") setAt(null); };
    const away = () => setAt(null);
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    window.addEventListener("resize", away);
    window.addEventListener("scroll", away, true);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("keydown", escape);
      window.removeEventListener("resize", away);
      window.removeEventListener("scroll", away, true);
    };
  }, [at]);
  if (!shown.some((item) => item !== "-")) return null;
  const open = () => {
    const box = button.current.getBoundingClientRect();
    const right = Math.max(8, window.innerWidth - box.right);
    // Open upwards when there is not enough room below.
    setAt(box.bottom + 260 > window.innerHeight ? { bottom: window.innerHeight - box.top + 4, right } : { top: box.bottom + 4, right });
  };
  return (
    <>
      <button ref={button} type="button" className="icon-button" title={label} aria-label={label} aria-haspopup="menu" aria-expanded={Boolean(at)} onClick={() => (at ? setAt(null) : open())}>
        <MoreHorizontal size={16} />
      </button>
      {at && (
        <div ref={menu} className="action-menu" role="menu" aria-label={label} style={at}>
          {shown.map((item, index) => item === "-" ? <hr key={`divider-${index}`} /> : (
            <button key={item.label} type="button" role="menuitem" className={item.danger ? "danger" : ""} onClick={() => { setAt(null); item.onClick(); }}>
              {item.icon}
              {item.label}
            </button>
          ))}
        </div>
      )}
    </>
  );
}
