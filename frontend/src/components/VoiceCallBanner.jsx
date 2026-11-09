import { useMicActivity } from "../lib/useMicActivity";
import ScreenStage, { ScreenShareButton } from "./ScreenStage";
import { useEffect, useState } from "react";
import {
  AudioLines,
  Headphones,
  HeadphoneOff,
  LockKeyhole,
  Mic,
  MicOff,
  PhoneIncoming,
  PhoneOff,
} from "lucide-react";
export default function VoiceCallBanner({ voice, users = [], voiceRoom }) {
  const speaking = useMicActivity(voice?.micStream, Boolean(voice?.call && voice.call.status !== "incoming" && !voice.muted && !voice.deafened));
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!voice?.call?.startedAt) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [voice?.call?.startedAt]);
  if (!voice?.call) return null;
  const call = voice.call,
    elapsed = call.startedAt
      ? Math.max(0, Math.floor((now - call.startedAt) / 1000))
      : 0;
  const duration = `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, "0")}`;
  const peerName = users.find((user) => user._id === call.peer)?.fullName || call.name || "Your contact";
  return (
    <>
    <ScreenStage screen={voice.screen} nameOf={() => peerName} deafened={voice.deafened} />
    {voice.waiting && (
      <section className="call-banner call-waiting" aria-label="Second incoming call">
        <PhoneIncoming size={23} />
        <div>
          <strong>Incoming call from {users.find((user) => user._id === voice.waiting.peer)?.fullName || "someone"}</strong>
          <small>Swapping ends your current call</small>
        </div>
        <button className="primary compact" onClick={voice.swapCall}>Swap</button>
        <button className="end-call" title="Decline call" aria-label="Decline waiting call" onClick={voice.declineWaiting}>
          <PhoneOff size={18} />
        </button>
      </section>
    )}
    <section className="call-banner" aria-label="Voice call">
      <AudioLines size={23} />
      <div>
        <strong>
          {
            {
              incoming: "Incoming voice call",
              calling: "Calling...",
              connecting: "Connecting voice...",
              connected: "Voice connected",
              reconnecting: "Reconnecting voice...",
            }[call.status]
          }
        </strong>
        <small>
          {users.find((user) => user._id === call.peer)?.fullName ||
            call.name ||
            "Sonar member"}
          {call.startedAt ? ` · ${duration}` : ""} · {call.status === "incoming" && voiceRoom?.room ? "Swapping leaves the voice room" : "No recordings"}
        </small>
      </div>
      <span className="voice-transport">
        <LockKeyhole size={11} />
        {call.relayOnly ? "Relay only" : "Encrypted audio"}
      </span>
      {voice.playbackBlocked && (
        <button className="secondary compact" onClick={voice.resumeAudio}>
          Enable sound
        </button>
      )}
      {call.status === "incoming" ? (
        <button
          className="primary compact"
          title={voiceRoom?.room ? "Swapping leaves the voice room" : undefined}
          onClick={() => { if (voiceRoom?.room) voiceRoom.leave(); voice.accept(); }}
        >
          {voiceRoom?.room ? "Swap" : "Accept"}
        </button>
      ) : (
        <>
          <button
            className={`icon-button ${voice.muted ? "accent" : ""} ${speaking ? "mic-speaking" : ""}`}
            aria-label={voice.muted ? "Unmute microphone" : "Mute microphone"}
            title={voice.muted ? "Unmute microphone" : "Mute microphone"}
            onClick={voice.toggleMute}
          >
            {voice.muted ? <MicOff size={18} /> : <Mic size={18} />}
          </button>
          <button
            className={`icon-button ${voice.deafened ? "accent" : ""}`}
            aria-label={voice.deafened ? "Undeafen" : "Deafen"}
            title={voice.deafened ? "Undeafen" : "Deafen"}
            onClick={voice.toggleDeafen}
          >
            {voice.deafened ? (
              <HeadphoneOff size={18} />
            ) : (
              <Headphones size={18} />
            )}
          </button>
          {(call.status === "connected" || call.status === "reconnecting") && <ScreenShareButton screen={voice.screen} />}
        </>
      )}
      <button
        className="end-call"
        title={call.status === "incoming" ? "Decline call" : "End call"}
        aria-label={call.status === "incoming" ? "Decline call" : "End call"}
        onClick={voice.hangup}
      >
        <PhoneOff size={18} />
      </button>
    </section>
    </>
  );
}
