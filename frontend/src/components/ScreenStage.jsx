import { useEffect, useRef, useState } from "react";
import { Clapperboard, LogOut, Maximize, MessageSquare, ScreenShare, ScreenShareOff, Volume2 } from "lucide-react";
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

function RemoteScreen({ stream, name, deafened, label, theater, onView, onLeave }) {
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
  return (
    <figure className="screen-tile">
      <video ref={video} autoPlay playsInline aria-label={label ? `${name}'s TV: ${label}` : `${name}'s screen`} />
      <figcaption>
        <span>{label ? `${name} is sharing TV: ${label}` : `${name}'s screen`}</span>
        <label className="screen-volume" title="Volume of this stream">
          <Volume2 size={15} />
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={volume}
            aria-label={`Volume of ${name}'s screen`}
            onChange={(event) => setVolume(Number(event.target.value))}
          />
        </label>
        {onView && (
          <button className="icon-button" aria-label={theater ? "Chat view" : "Theater view"} title={theater ? "Chat view: show the chat" : "Theater view"} onClick={onView}>
            {theater ? <MessageSquare size={16} /> : <Clapperboard size={16} />}
          </button>
        )}
        {onLeave && (
          <button className="icon-button" aria-label="Stop watching" title="Stop watching (you can join again)" onClick={onLeave}>
            <LogOut size={16} />
          </button>
        )}
        <button className="icon-button" aria-label="Full screen" title="Full screen" onClick={() => video.current.requestFullscreen?.()}>
          <Maximize size={16} />
        </button>
      </figcaption>
    </figure>
  );
}

// Screens other people are sharing with us, in a call or a voice room.
export default function ScreenStage({ screen, nameOf, deafened }) {
  // TV opens in theater view (the chat hidden); the button switches to the chat and back.
  const [theater, setTheater] = useState(true);
  const entries = Object.entries(screen?.remotes || {});
  if (!entries.length) return null;
  const labels = screen?.labels || {};
  const tv = entries.some(([id]) => labels[id]);
  return (
    <aside className={`screen-stage ${tv && theater ? "theater" : ""}`} aria-label="Shared screens">
      {entries.map(([id, stream]) => (
        <RemoteScreen key={id} stream={stream} name={nameOf(id)} deafened={deafened} label={labels[id]} theater={theater} onView={labels[id] ? () => setTheater((value) => !value) : null} onLeave={labels[id] && screen.leave ? () => screen.leave(id) : null} />
      ))}
    </aside>
  );
}
