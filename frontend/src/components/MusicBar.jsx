import { useEffect, useRef, useState } from "react";
import { Music, Volume2 } from "lucide-react";
import { applyScreenOutput } from "../lib/useScreenShare";

function Playing({ stream, name, title, deafened }) {
  const audio = useRef(null);
  const [volume, setVolume] = useState(1);
  useEffect(() => { if (audio.current) audio.current.volume = volume; }, [volume]);
  useEffect(() => { if (audio.current) audio.current.muted = Boolean(deafened); }, [deafened]);
  useEffect(() => {
    const player = audio.current;
    if (!player || !stream) return undefined;
    player.srcObject = stream;
    applyScreenOutput(player).catch(() => {});
    player.play().catch(() => {});
    return () => { player.srcObject = null; };
  }, [stream]);
  return (
    <div className="music-bar" aria-label={`${name} is playing ${title}`}>
      <span className="music-icon"><Music size={16} /></span>
      <span className="music-text"><small>{name} is playing</small><strong>{title}</strong></span>
      <label className="screen-volume" title="Your volume for this music">
        <Volume2 size={15} />
        <input type="range" min="0" max="1" step="0.01" value={volume} aria-label="Music volume" onChange={(event) => setVolume(Number(event.target.value))} />
      </label>
      <audio ref={audio} autoPlay />
    </div>
  );
}

// Listen together: music someone plays from their PC for everyone in the call. It plays
// straight away (no join step); each listener has their own volume. Same as the desktop app.
export default function MusicBar({ screen, nameOf, deafened }) {
  const playing = Object.entries(screen?.music || {});
  if (!playing.length) return null;
  return (
    <div className="music-bars">
      {playing.map(([id, title]) => <Playing key={id} stream={screen.remotes?.[id]} name={nameOf(id)} title={title} deafened={deafened} />)}
    </div>
  );
}
