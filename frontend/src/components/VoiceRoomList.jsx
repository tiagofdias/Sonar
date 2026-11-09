import { useEffect } from "react";
import { Lock, LogOut, MicOff, Plus, Volume2 } from "lucide-react";
import toast from "react-hot-toast";
import { useAuthStore } from "../store/useAuthStore";
export default function VoiceRoomList({
  community,
  voice,
  voiceRoom,
  onCreate,
}) {
  const { authUser, socket, presence } = useAuthStore();
  useEffect(() => {
    const load = () => voiceRoom.observe(community._id);
    if (socket?.connected) load();
    socket?.on("connect", load);
    return () => socket?.off("connect", load);
  }, [community._id, socket]);
  const owner = community.ownerId === authUser._id;
  const moderator = community.moderators?.includes(authUser._id);
  const channels = community.channels.filter(
    (channel) => channel.type === "voice",
  );
  return (
    <div className="voice-room-list">
      <div className="sidebar-section">
        <span>VOICE ROOMS</span>
        {community.ownerId === authUser._id && (
          <button
            className="icon-button"
            title="Create a voice room"
            onClick={onCreate}
          >
            <Plus size={16} />
          </button>
        )}
      </div>
      {!channels.length && (
        <p className="voice-room-empty">
          {community.ownerId === authUser._id
            ? "Make room for a real conversation. Create your first voice room."
            : "No voice rooms yet. The owner can create one."}
        </p>
      )}
      {channels.map((channel) => {
        const active =
          voiceRoom.room?.communityId === community._id &&
          voiceRoom.room?.channelId === channel._id;
        const snapshot = voiceRoom.rooms[`${community._id}:${channel._id}`],
          participants = snapshot?.participants || [];
        return (
          <div className="voice-room-entry" key={channel._id}>
            <button
              className={`nav-row ${active ? "selected" : ""}`}
              aria-label={
                active
                  ? `Connected to ${channel.name}`
                  : `Join voice room ${channel.name}`
              }
              disabled={active || channel.locked}
              onClick={() => {
                if (voice.call)
                  return toast(
                    "End your direct call before joining a voice room.",
                  );
                voiceRoom.join(community, channel);
              }}
            >
              <Volume2 size={17} />
              {channel.name}
              {channel.locked && <Lock size={13} aria-label="Locked to new participants" />}
              <small>{participants.length}/6</small>
            </button>
            {participants.length > 0 && (
              <ul className="room-participants">
                {participants.map((person) => (
                  <li key={person.sessionId}>
                    <span className={["away", "busy"].includes(presence[person.userId]) ? `status-dot status-${presence[person.userId]}` : "green-dot"} />
                    <span>
                      {person.name}
                      {person.userId === authUser._id ? " (you)" : ""}
                    </span>
                    {(owner || moderator) && person.userId !== authUser._id && person.userId !== community.ownerId && (owner || !community.moderators?.includes(person.userId)) &&
                      <button className="room-kick" aria-label={`Disconnect ${person.name} from voice`} title="Disconnect from voice" onClick={() => {
                        if (window.confirm(`Disconnect ${person.name} from this voice room? They can rejoin unless the room is locked.`))
                          voiceRoom.kick(community._id, channel._id, person);
                      }}><LogOut size={12} /></button>}
                    {(person.muted || person.deafened) && (
                      <MicOff size={11} aria-label="Microphone muted" />
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </div>
  );
}
