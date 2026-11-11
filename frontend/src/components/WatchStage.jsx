import { useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowUp, ListVideo, Pause, Play, Plus, SkipForward, Trash2, X } from "lucide-react";
import Avatar from "./Avatar";
import ActionMenu from "./ActionMenu";
import { createYouTubeFrame, PLAYER_STATE } from "../lib/youtubeFrame";
import { CONTINUE_AFTER_MS, REACTIONS, STALL_MS, drifted, expectedPosition, formatTime, shouldPlay, waitingNames } from "../lib/watch";

const STATE_LABEL = { joining: "Joining", watching: "Ready", waiting: "Ad or loading", "catching-up": "Catching up" };

// Watch Together: one YouTube player kept in step with everyone else in the call.
// The player exists only while this stage is shown and is fully removed after.
export default function WatchStage({ watch, me, standalone = false }) {
  const { state } = watch;
  const host = useRef(null);
  const player = useRef(null);
  const loaded = useRef(null);
  const time = useRef({ value: 0, moved: 0 });
  const playerState = useRef(PLAYER_STATE.unstarted);
  const commanded = useRef(0);
  const reported = useRef(null);
  const latest = useRef(state);
  const actions = useRef(watch);
  latest.current = state;
  actions.current = watch;
  const [duration, setDuration] = useState(0);
  const [, setTick] = useState(0);
  const [link, setLink] = useState("");
  const [adding, setAdding] = useState(false);
  const [dragging, setDragging] = useState(null);
  const [stuck, setStuck] = useState(false);
  // While dragging the bar nothing is sent; letting go jumps everyone once.
  const [scrub, setScrub] = useState(null);
  const commitScrub = () => {
    if (scrub === null) return;
    actions.current.seek(scrub);
    setScrub(null);
  };
  const item = state?.queue?.[state.current];

  const command = (run) => { commanded.current = Date.now(); run(); };
  const report = (value) => {
    if (reported.current === value) return;
    reported.current = value;
    actions.current.status(value);
  };

  // One player per session: later videos load inside it, and it is removed on leave.
  useEffect(() => {
    if (!item) {
      player.current?.destroy();
      player.current = null;
      loaded.current = null;
      return;
    }
    if (loaded.current === item.id) return;
    const start = expectedPosition(latest.current, actions.current.offset.current);
    time.current = { value: start, moved: Date.now() };
    reported.current = null;
    setDuration(0);
    if (player.current) command(() => player.current.load(item.videoId, start));
    else player.current = createYouTubeFrame(host.current, {
      videoId: item.videoId,
      start,
      onDuration: setDuration,
      onTime: (value) => {
        if (Math.abs(value - time.current.value) > 0.05) time.current.moved = Date.now();
        time.current.value = value;
      },
      onState: (value) => {
        const before = playerState.current;
        playerState.current = value;
        const current = latest.current;
        if (!current || before === value) return;
        if (value === PLAYER_STATE.ended) {
          const playing = current.queue[current.current];
          if (playing) actions.current.ended(playing.id);
          return;
        }
        // Play or pause pressed on YouTube's own controls applies to everyone.
        if (Date.now() - commanded.current < 1500) return;
        if (value === PLAYER_STATE.paused && current.playing && !current.hold) actions.current.pause();
        if (value === PLAYER_STATE.playing && !current.playing) actions.current.play();
      },
    });
    loaded.current = item.id;
  }, [item?.id, item?.videoId]);
  useEffect(() => () => { player.current?.destroy(); player.current = null; }, []);

  // Follow every shared change: play, pause, a hold, or a jump.
  useEffect(() => {
    const p = player.current;
    if (!p || !state || !item) return;
    const want = shouldPlay(state, me);
    command(want ? p.play : p.pause);
    const expected = expectedPosition(state, watch.offset.current);
    if (!state.hold?.waitingFor.includes(me) && drifted(time.current.value, expected, 1.5)) command(() => p.seek(expected));
  }, [state]);

  // Once a second: report whether this video is moving, and correct drift.
  useEffect(() => {
    const timer = setInterval(() => {
      setTick((n) => n + 1);
      const current = latest.current, p = player.current;
      if (!current || !p || !current.queue[current.current] || !p.ready) return;
      const want = shouldPlay(current, me);
      const moving = Date.now() - time.current.moved < STALL_MS;
      if (want && !moving) report("waiting");
      else report("watching");
      // A browser that blocked autoplay needs one click here.
      setStuck(want && !moving && [PLAYER_STATE.unstarted, PLAYER_STATE.cued, PLAYER_STATE.paused].includes(playerState.current));
      if (want && moving && !current.hold) {
        const expected = expectedPosition(current, actions.current.offset.current);
        if (drifted(time.current.value, expected)) command(() => p.seek(expected));
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [me]);

  if (!state) return <section className="watch-stage loading" aria-label="Watch together">Joining watch together…</section>;
  const position = expectedPosition(state, watch.offset.current);
  const waitingOnMe = state.hold?.waitingFor.includes(me);
  const heldFor = state.hold ? Date.now() + watch.offset.current - state.hold.since : 0;
  const submit = async (event) => {
    event.preventDefault();
    if (!link.trim() || adding) return;
    setAdding(true);
    if (await watch.add(link.trim())) setLink("");
    setAdding(false);
  };

  return (
    <section className={`watch-stage ${standalone ? "standalone" : ""}`} aria-label="Watch together">
      <div className="watch-main">
        <div className="watch-player">
          <div className="watch-frame" ref={host} />
          {!item && (
            <div className="watch-empty">
              <ListVideo size={34} />
              <strong>Paste a YouTube link to start</strong>
              <small>Everyone here watches it together, in sync.</small>
            </div>
          )}
          {state.hold && (
            <div className="watch-hold" role="status">
              {waitingOnMe ? <strong>Everyone is waiting for your video to start…</strong> : <strong>Waiting for {waitingNames(state)}</strong>}
              <small>{waitingOnMe ? "An ad or slow loading is holding your video." : "An ad or slow loading is holding their video."}</small>
              {!waitingOnMe && heldFor > CONTINUE_AFTER_MS && <button className="secondary compact" onClick={watch.continueWithout}>Continue without {waitingNames(state)}</button>}
            </div>
          )}
          {stuck && !state.hold && (
            <button className="watch-start primary" onClick={() => command(player.current.play)}><Play size={18} /> Start the video</button>
          )}
          <div className="watch-reactions" aria-hidden="true">
            {watch.reactions.map((reaction) => <span key={reaction.id} style={{ left: `${reaction.left}%` }}>{reaction.emoji}</span>)}
          </div>
        </div>
        <div className="watch-controls">
          <button className="icon-button" aria-label={state.playing ? "Pause for everyone" : "Play for everyone"} title={state.playing ? "Pause for everyone" : "Play for everyone"} disabled={!item} onClick={state.playing ? watch.pause : watch.play}>
            {state.playing ? <Pause size={18} /> : <Play size={18} />}
          </button>
          <button className="icon-button" aria-label="Skip to the next video" title="Skip to the next video" disabled={!item} onClick={watch.skip}><SkipForward size={18} /></button>
          <input className="watch-seek" type="range" min="0" max={Math.max(1, Math.floor(duration || position + 1))} step="1" value={scrub ?? Math.floor(position)} disabled={!item || !duration} aria-label="Position"
            onChange={(event) => setScrub(Number(event.target.value))}
            onPointerUp={commitScrub} onKeyUp={commitScrub} onBlur={commitScrub} />
          <span className="watch-time">{formatTime(position)}{duration ? ` / ${formatTime(duration)}` : ""}</span>
          <span className="watch-react">
            {REACTIONS.map((emoji) => <button key={emoji} type="button" aria-label={`React ${emoji}`} onClick={() => watch.react(emoji)}>{emoji}</button>)}
          </span>
          {!standalone && <button className="icon-button" aria-label="Stop watching" title="Stop watching" onClick={watch.leave}><X size={18} /></button>}
        </div>
        <div className="watch-viewers" aria-label="Who is watching">
          {state.viewers.map((viewer) => (
            <span key={viewer.userId} className={`watch-viewer state-${viewer.state}`} title={`${viewer.name}: ${STATE_LABEL[viewer.state] || viewer.state}`}>
              <Avatar user={{ _id: viewer.userId, animal: viewer.animal }} />
              <span><strong>{viewer.userId === me ? "You" : viewer.name}</strong><small>{STATE_LABEL[viewer.state] || viewer.state}</small></span>
            </span>
          ))}
        </div>
      </div>
      <aside className="watch-queue" aria-label="Up next">
        <form onSubmit={submit}>
          <input value={link} onChange={(event) => setLink(event.target.value)} placeholder={state.playlists ? "YouTube video or playlist link" : "YouTube video link"} aria-label="YouTube link" maxLength={500} />
          <button className="primary compact" disabled={!link.trim() || adding} aria-label="Add to the queue"><Plus size={16} /></button>
        </form>
        <ol>
          {state.queue.map((entry, index) => (
            <li key={entry.id} className={`${index === state.current ? "current" : ""} ${dragging === entry.id ? "dragging" : ""}`} draggable
              onDragStart={() => setDragging(entry.id)} onDragEnd={() => setDragging(null)}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => { event.preventDefault(); if (dragging && dragging !== entry.id) watch.move(dragging, index); setDragging(null); }}>
              <button type="button" className="watch-item" onClick={() => index !== state.current && watch.playItem(entry.id)} title={index === state.current ? "Playing now" : "Play now"}>
                <img src={entry.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
                <span><strong>{entry.title}</strong><small>{index === state.current ? "Playing now" : `Added by ${entry.addedByName}`}</small></span>
              </button>
              <ActionMenu label={`Options for ${entry.title}`} items={[
                index !== state.current && { label: "Play now", icon: <Play size={15} />, onClick: () => watch.playItem(entry.id) },
                index > 0 && { label: "Move up", icon: <ArrowUp size={15} />, onClick: () => watch.move(entry.id, index - 1) },
                index < state.queue.length - 1 && { label: "Move down", icon: <ArrowDown size={15} />, onClick: () => watch.move(entry.id, index + 1) },
                "-",
                { label: "Remove from the queue", icon: <Trash2 size={15} />, danger: true, onClick: () => watch.remove(entry.id) },
              ]} />
            </li>
          ))}
        </ol>
        {state.segments?.length > 0 && <small className="watch-credit">Sponsor segments skipped using SponsorBlock data (CC BY-NC-SA 4.0).</small>}
      </aside>
    </section>
  );
}
