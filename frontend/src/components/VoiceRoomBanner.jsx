import { useMicActivity } from "../lib/useMicActivity";
import { roomStatus } from "../lib/roomStatus";
import ScreenStage, { ScreenShareButton } from "./ScreenStage";
import AppsMenu from "./AppsMenu";
import WatchStage from "./WatchStage";
import { useAuthStore } from "../store/useAuthStore";
import {
  Headphones,
  HeadphoneOff,
  LockKeyhole,
  LogOut,
  Mic,
  MicOff,
  Volume2,
} from "lucide-react";
export default function VoiceRoomBanner({ voiceRoom, watch }) {
  const me = useAuthStore((store) => store.authUser?._id);
  const room = voiceRoom?.room;
  const speaking = useMicActivity(voiceRoom?.micStream, Boolean(room && !voiceRoom.muted && !voiceRoom.deafened));
  if (!room) return null;
  const others = (room.participants || []).filter((person) => person.sessionId !== room.sessionId);
  const states = others.map((person) => voiceRoom.connections[person.userId]);
  const status = roomStatus({
    joining: room.status === "joining",
    others: others.length,
    connected: states.filter((value) => value === "connected").length,
    failed: states.filter((value) => value === "failed").length,
  });
  const nameOf = (id) => room.participants?.find((person) => person.userId === id)?.name || "Member";
  return (
    <>
    <ScreenStage screen={voiceRoom.screen} nameOf={nameOf} deafened={voiceRoom.deafened} />
    {watch?.joined && room.status === "joined" && <WatchStage watch={watch} me={me} />}
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
          {room.communityName} · {room.participants.length}/6 people · {status}
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
      {room.status === "joined" && <AppsMenu watch={watch} screen={voiceRoom.screen} nameOf={nameOf} />}
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
