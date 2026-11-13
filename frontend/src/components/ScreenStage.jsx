import { useEffect, useRef, useState } from "react";
import { Grip, LogOut, Maximize, Minimize, MoveDiagonal2, PictureInPicture2, ScreenShare, ScreenShareOff, Volume2 } from "lucide-react";
import { applyScreenOutput, SCREEN_QUALITIES } from "../lib/useScreenShare";

export function ScreenShareButton({ screen }) {
  const [choosing, setChoosing] = useState(false);
  if (!screen?.supported) return null;
  if (screen.sharing)
    return (
      <button className="icon-button accent" aria-label="Stop sharing your screen" title="Stop sharing your screen" onClick={screen.stop}>
        <ScreenShareOff size={18} />
      </button>
    );
  return (
    <span className="screen-share-menu">
      <button className="icon-button" aria-label="Share your screen" title="Share your screen" aria-expanded={choosing} onClick={() => setChoosing(!choosing)}>
        <ScreenShare size={18} />
      </button>
      {choosing && (
        <span className="screen-share-options" role="menu">
          {Object.entries(SCREEN_QUALITIES).map(([height, quality]) => (
            <button key={height} role="menuitem" onClick={() => { setChoosing(false); screen.start(Number(height)); }}>
              Share at {quality.label}
            </button>
          ))}
        </span>
      )}
    </span>
  );
}

// Where the floating player sits: left, top and width, kept in this browser (like the desktop app keeps it).
const PLAYER_KEY = "sonar.player";
const MIN_WIDTH = 320;
const CAPTION = 42;
const loadBox = () => {
  try { const box = JSON.parse(localStorage.getItem(PLAYER_KEY)); return Array.isArray(box) && box.length === 3 && box.every(Number.isFinite) ? box : null; } catch { return null; }
};
const saveBox = (box) => { try { localStorage.setItem(PLAYER_KEY, JSON.stringify(box)); } catch { /* private mode */ } };

function RemoteScreen({ stream, name, deafened, label, height, expanded, onGrab, onExpand, onLeave, onRatio }) {
  const video = useRef(null);
  const [volume, setVolume] = useState(1);
  useEffect(() => { video.current.volume = volume; }, [volume]);
  useEffect(() => {
    const player = video.current;
    player.srcObject = stream;
    applyScreenOutput(player).catch(() => {});
    player.play().catch(() => {});
    return () => { player.srcObject = null; };
  }, [stream]);
  useEffect(() => { video.current.muted = Boolean(deafened); }, [deafened]);
  const caption = label ? `${name} is sharing TV: ${label}` : `${name}'s screen`;
  const bar = (
    <figcaption>
      <span className="screen-grab" onPointerDown={expanded ? undefined : onGrab} title={expanded ? undefined : "Drag to move"}>
        {!expanded && <Grip size={14} />}
        <span>{caption}</span>
      </span>
      {onLeave && (
        <button className="secondary compact screen-leave" aria-label="Stop watching" title="Stop watching (you can join again)" onClick={onLeave}>
          <LogOut size={14} /> Leave
        </button>
      )}
      <label className="screen-volume" title="Volume of this stream">
        <Volume2 size={15} />
        <input type="range" min="0" max="1" step="0.01" value={volume} aria-label={`Volume of ${name}'s screen`} onChange={(event) => setVolume(Number(event.target.value))} />
      </label>
      {document.pictureInPictureEnabled && (
        <button className="icon-button" aria-label="Picture-in-picture" title="Picture-in-picture: a small window on top of everything" onClick={() => (document.pictureInPictureElement === video.current ? document.exitPictureInPicture() : video.current.requestPictureInPicture()).catch(() => {})}>
          <PictureInPicture2 size={16} />
        </button>
      )}
      <button className="icon-button" aria-label={expanded ? "Back to the small player" : "Fill the window"} title={expanded ? "Back to the small player" : "Fill the window"} onClick={onExpand}>
        {expanded ? <Minimize size={16} /> : <Maximize size={16} />}
      </button>
    </figcaption>
  );
  return (
    <figure className={`screen-tile ${expanded ? "expanded" : ""}`}>
      {expanded && bar}
      <video ref={video} autoPlay playsInline aria-label={label ? `${name}'s TV: ${label}` : `${name}'s screen`} style={expanded ? undefined : { height }}
        onLoadedMetadata={(event) => onRatio(event.currentTarget.videoHeight / Math.max(1, event.currentTarget.videoWidth))}
        onResize={(event) => onRatio(event.currentTarget.videoHeight / Math.max(1, event.currentTarget.videoWidth))} />
      {!expanded && bar}
    </figure>
  );
}

// Screens and TV other people share with us: a floating player, the same as in the desktop app.
// Drag it by its caption, resize it from the corner grip; it stays where it is put.
export default function ScreenStage({ screen, nameOf, deafened }) {
  const [box, setBox] = useState(loadBox);
  const [expanded, setExpanded] = useState(null);
  const [ratios, setRatios] = useState({});
  const [view, setView] = useState({ width: window.innerWidth, height: window.innerHeight });
  const drag = useRef(null);
  useEffect(() => {
    const resize = () => setView({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  // Music has no picture: it plays in the "now playing" bar instead (MusicBar).
  const entries = Object.entries(screen?.remotes || {}).filter(([id]) => !screen?.music?.[id]);
  const labels = screen?.labels || {};
  const heightFor = (width) => entries.reduce((sum, [id]) => sum + width * (ratios[id] || 9 / 16) + CAPTION, 0) + 10 * Math.max(0, entries.length - 1);
  // The same rules as the desktop app: at least 320 px wide, always inside the window.
  const place = ([x, y, width]) => {
    const w = Math.min(Math.max(width, MIN_WIDTH), Math.max(MIN_WIDTH, view.width - 20));
    return [Math.min(Math.max(x, 0), Math.max(0, view.width - w)), Math.min(Math.max(y, 0), Math.max(0, view.height - Math.min(heightFor(w), view.height))), w];
  };
  const [x, y, width] = place(box || [view.width - 520 - 18, view.height - heightFor(520) - 18, 520]);
  const grab = (resize) => (event) => {
    if (event.button !== 0 || event.target.closest("button, input, label")) return;
    event.preventDefault();
    drag.current = { resize, from: [event.clientX, event.clientY], start: [x, y, width] };
    const move = (moved) => {
      const { from, start } = drag.current;
      const [dx, dy] = [moved.clientX - from[0], moved.clientY - from[1]];
      setBox(place(resize ? [start[0], start[1], start[2] + dx] : [start[0] + dx, start[1] + dy, start[2]]));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      drag.current = null;
      setBox((current) => { if (current) saveBox(current); return current; });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  if (!entries.length) return null;
  const shown = expanded && screen.remotes[expanded] ? entries.filter(([id]) => id === expanded) : entries;
  return (
    <aside className={`screen-stage ${expanded && screen.remotes[expanded] ? "expanded" : ""}`} aria-label="Shared screens"
      style={expanded && screen.remotes[expanded] ? undefined : { left: x, top: y, width }}>
      {shown.map(([id, stream]) => (
        <RemoteScreen key={id} stream={stream} name={nameOf(id)} deafened={deafened} label={labels[id]}
          height={width * (ratios[id] || 9 / 16)} expanded={expanded === id}
          onGrab={grab(false)} onExpand={() => setExpanded(expanded === id ? null : id)}
          onLeave={labels[id] && screen.leave ? () => screen.leave(id) : null}
          onRatio={(ratio) => ratio > 0 && setRatios((all) => (all[id] === ratio ? all : { ...all, [id]: ratio }))} />
      ))}
      {!(expanded && screen.remotes[expanded]) && (
        <span className="screen-resize" onPointerDown={grab(true)} title="Drag to resize" aria-hidden="true"><MoveDiagonal2 size={13} /></span>
      )}
    </aside>
  );
}
