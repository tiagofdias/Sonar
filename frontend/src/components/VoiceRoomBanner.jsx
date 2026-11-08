import { useMicActivity } from "../lib/useMicActivity";
import ScreenStage, { ScreenShareButton } from "./ScreenStage";
import {
  Headphones,
  HeadphoneOff,
  LockKeyhole,
  LogOut,
  Mic,
  MicOff,
  Volume2,
} from "lucide-react";
export default function VoiceRoomBanner({ voiceRoom }) {
  const room = voiceRoom?.room;
  const speaking = useMicActivity(voiceRoom?.micStream, Boolean(room && !voiceRoom.muted && !voiceRoom.deafened));
  if (!room) return null;
  const connected = Object.values(voiceRoom.connections).filter(
    (value) => value === "connected",
  ).length;
  const nameOf = (id) => room.participants?.find((person) => person.userId === id)?.fullName || "Member";
  return (
    <>
    <ScreenStage screen={voiceRoom.screen} nameOf={nameOf} deafened={voiceRoom.deafened} />
    <section
      className="call-banner room-call-banner"
      aria-label="Community voice room"
    >
      <Volume2 size={23} />
      <div>
        <strong>
          {room.status === "joining"
            ? "Joining voice room..."
            : `Voice room: ${room.name}`}
        </strong>
        <small>
          {room.communityName} · {room.participants.length}/6 people ·{" "}
          {connected} peer {connected === 1 ? "connection" : "connections"}
        </small>
      </div>
      <span className="voice-transport">
        <LockKeyhole size={11} />
        {room.relayOnly ? "Relay only" : "Encrypted audio"}
      </span>
      {voiceRoom.playbackBlocked && (
        <button className="secondary compact" onClick={voiceRoom.resumeAudio}>
          Enable room sound
        </button>
      )}
      <button
        className={`icon-button ${voiceRoom.muted ? "accent" : ""} ${speaking ? "mic-speaking" : ""}`}
        aria-label={
          voiceRoom.muted ? "Unmute room microphone" : "Mute room microphone"
        }
        onClick={voiceRoom.toggleMute}
      >
        {voiceRoom.muted ? <MicOff size={18} /> : <Mic size={18} />}
      </button>
      <button
        className={`icon-button ${voiceRoom.deafened ? "accent" : ""}`}
        aria-label={voiceRoom.deafened ? "Undeafen room" : "Deafen room"}
        onClick={voiceRoom.toggleDeafen}
      >
        {voiceRoom.deafened ? (
          <HeadphoneOff size={18} />
        ) : (
          <Headphones size={18} />
        )}
      </button>
      <ScreenShareButton screen={voiceRoom.screen} />
      <button
        className="end-call"
        aria-label="Leave voice room"
        title="Leave voice room"
        onClick={voiceRoom.leave}
      >
        <LogOut size={18} />
      </button>
    </section>
    </>
  );
}
