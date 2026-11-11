import PresenceControl from "./PresenceControl";
import { useEffect, useState, useRef } from "react";
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  Copy,
  Hash,
  Lock,
  LockOpen,
  LockKeyhole,
  LogOut,
  MessageCircle,
  Pin,
  Plus,
  Pencil,
  Search,
  Ban,
  BellOff,
  Bell,
  Send,
  Reply,
  ChevronDown,
  Crown,
  Settings,
  ShieldCheck,
  Timer,
  Trash2,
  UserPlus,
  Users,
  X,
} from "lucide-react";
import toast from "react-hot-toast";
import { useAuthStore } from "../store/useAuthStore";
import { useCommunityStore } from "../store/useCommunityStore";
import { useUnreadStore } from "../store/useUnreadStore";
import MessageText from "./MessageText";
import ComposerTools, { FileCard } from "./ComposerTools";
import { fileLabel } from "../lib/messageContent";
import { useConversationDraft } from "../store/useDraftStore";
import VoiceCallBanner from "./VoiceCallBanner";
import ReactionBar from "./ReactionBar";
import { reactionSummary } from "../lib/reactions";
import VoiceRoomBanner from "./VoiceRoomBanner";
import VoiceRoomList from "./VoiceRoomList";
import { useHistoryScroll } from "../lib/useHistoryScroll";
import Avatar from "./Avatar";
import ActionMenu from "./ActionMenu";
import SearchPanel from "./SearchPanel";
import { usePreferencesStore } from "../store/usePreferencesStore";
import { useTyping, typingText } from "../lib/useTyping";
import { completeMention, mentionQuery, mentionsIn } from "../lib/mentions";
const PERMISSIONS = [["pin", "Pin other people's messages"], ["delete", "Delete other people's messages"], ["remove", "Remove and ban members"], ["lock", "Lock voice rooms"]];
const initials = (name) =>
  name
    ?.trim()
    .split(/\s+/)
    .map((part) => part[0])
    .slice(0, 2)
    .join("")
    .toUpperCase() || "?";

export default function CommunityWorkspace({
  onDirectMessages,
  voice,
  voiceRoom,
  watch,
  hasUnreadDirect,
}) {
  const { authUser, socket, onlineUsers, logout } = useAuthStore();
  const store = useCommunityStore();
  const messageBox = useRef(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef(null);
  useEffect(() => {
    if (!menuOpen) return undefined;
    const away = (event) => { if (!menuRef.current?.contains(event.target)) setMenuOpen(false); };
    const escape = (event) => { if (event.key === "Escape") setMenuOpen(false); };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("mousedown", away); document.removeEventListener("keydown", escape); };
  }, [menuOpen]);
  const { communities, community, channel, messages, loading } = store;
  const unreadChannels = useUnreadStore(state => state.channels);
  const [dialog, setDialog] = useState(null),
    [value, setValue] = useState(""),
    [busy, setBusy] = useState(false),
    [sending, setSending] = useState(false),
    [invite, setInvite] = useState(null),
    [pins, setPins] = useState(false),
    [membersOpen, setMembersOpen] = useState(false),
    [now, setNow] = useState(Date.now()),
    [showChannels, setShowChannels] = useState(false),
    [confirmation, setConfirmation] = useState(null),
    [channelType, setChannelType] = useState("text"),
    [renameChannel, setRenameChannel] = useState(""),
    [replyTo, setReplyTo] = useState(null),
    [bans, setBans] = useState(null),
    [editing, setEditing] = useState(null),
    [searching, setSearching] = useState(false),
    [activity, setActivity] = useState(null),
    [permissionsFor, setPermissionsFor] = useState(null),
    [denied, setDenied] = useState([]);
  const preferences = usePreferencesStore();
  const { typing, typed } = useTyping(socket, community && channel ? { communityId: community._id, channelId: channel._id } : null);
  const { text, setText, clearSent } = useConversationDraft(
    channel ? `channel:${community._id}:${channel._id}` : null,
  );
  const history = useHistoryScroll(messages, `channel:${channel?._id}:${pins}`);
  const open = (type) => {
    setValue("");
    setInvite(null);
    setDialog(type);
  };
  useEffect(() => {
    const token =
      new URLSearchParams(window.location.search).get("invite") ||
      sessionStorage.getItem("sonar-invite");
    if (token && /^[a-f0-9]{48}$/i.test(token)) {
      setDialog("join");
      setValue(token);
      sessionStorage.removeItem("sonar-invite");
      window.history.replaceState({}, "", window.location.pathname);
    }
  }, []);
  useEffect(() => {
    store.load();
    const selected = useCommunityStore.getState();
    if (selected.community && selected.channel) {
      const communityId = selected.community._id,
        selectedChannel = selected.channel;
      store.refresh().then((ok) => {
        const current = useCommunityStore.getState();
        const refreshedChannel = current.community?.channels.find(item => item._id === selectedChannel._id);
        if (ok && current.community?._id === communityId && refreshedChannel && current.channel?._id === selectedChannel._id)
          store.selectChannel(refreshedChannel);
      });
    }
    const receive = (message) => store.receive(message);
    const update = ({ communityId }) => {
      store.load();
      if (useCommunityStore.getState().community?._id === communityId)
        store.refresh();
    };
    const reconnect = () => {
      const current = useCommunityStore.getState();
      current.load();
      if (current.community) current.select(current.community._id);
    };
    const removed = ({ communityId, reason }) => {
      useUnreadStore.getState().clearCommunity(communityId);
      const selected = useCommunityStore.getState().communities.find(item => item._id === communityId);
      store.removeAccess(communityId);
      setDialog(null);
      setConfirmation(null);
      if (reason !== "deleted" || useAuthStore.getState().authUser?._id !== selected?.ownerId)
        toast(reason === "deleted" ? "This community was deleted." : "You no longer belong to that community");
    };
    const removeMessage = (event) => store.removeMessage(event);
    socket?.on("channelMessageDeleted", removeMessage);
    socket?.on("channelMessageNew", receive);
    socket?.on("channelMessage", receive);
    socket?.on("communityUpdated", update);
    socket?.on("communityRemoved", removed);
    socket?.on("connect", reconnect);
    const timer = setInterval(() => setNow(Date.now()), 15000);
    return () => {
      socket?.off("channelMessageDeleted", removeMessage);
      socket?.off("channelMessageNew", receive);
      socket?.off("channelMessage", receive);
      socket?.off("communityUpdated", update);
      socket?.off("communityRemoved", removed);
      socket?.off("connect", reconnect);
      clearInterval(timer);
    };
  }, [socket]);
  useEffect(() => {
    setPins(false);
    setReplyTo(null);
    setEditing(null);
    setShowChannels(false);
  }, [channel?._id]);
  useEffect(() => {
    setReplyTo(current => current && !messages.some(message => message._id === current._id) ? null : current);
  }, [messages]);
  useEffect(() => {
    const update = () => {
      useUnreadStore.getState().setActiveChannel(!document.hidden && !pins && community && channel ? `${community._id}:${channel._id}` : null);
      useCommunityStore.getState().recordVisibleChannel();
    };
    update();
    document.addEventListener("visibilitychange", update);
    return () => { document.removeEventListener("visibilitychange", update); useUnreadStore.getState().setActiveChannel(null); };
  }, [community?._id, channel?._id, pins]);
  useEffect(() => {
    if (!dialog && !confirmation) return;
    const escape = (event) => {
      if (event.key === "Escape" && !busy) {
        setDialog(null);
        setConfirmation(null);
      }
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [dialog, confirmation, busy]);
  const confirmMemberAction = async () => {
    if (busy || !confirmation || community?._id !== confirmation.communityId)
      return;
    setBusy(true);
    try {
      const success = confirmation.action === "delete-community"
        ? await store.deleteCommunity()
        : confirmation.action === "delete-channel"
        ? await store.deleteChannel(confirmation.channel._id)
        : await store.manageMember(confirmation.action, confirmation.member?._id);
      if (success) {
        toast.success(
          confirmation.action === "ban" ? "Member banned. Previous invite revoked." : confirmation.action === "unban" ? "Ban removed. Create a new invite to let them rejoin." : confirmation.action === "promote" ? "Moderator added" : confirmation.action === "demote" ? "Moderator removed" : confirmation.action === "delete-community" ? "Community deleted" : confirmation.action === "delete-channel" ? "Channel deleted" : confirmation.action === "leave"
            ? "You left the community"
            : confirmation.action === "remove"
              ? "Member removed. Previous invite revoked."
              : "Ownership transferred. Previous invite revoked.",
        );
        setConfirmation(null);
      }
    } finally {
      setBusy(false);
    }
  };
  const owner = community?.ownerId === authUser._id;
  const moderator = community?.moderators?.includes(authUser._id) || false;
  const group = community?.kind === "group";
  const may = (permission) => owner || (moderator && !community?.moderatorLimits?.find((limit) => limit.userId === authUser._id)?.denied.includes(permission));
  const canDisappear = group || owner || moderator;
  const members = community?.memberDetails || [];
  const typingMention = mentionQuery(text);
  const mentionOptions = typingMention === null ? [] : [
    ...((owner || moderator) && "everyone".startsWith(typingMention.toLowerCase()) ? [{ _id: "everyone", fullName: "everyone" }] : []),
    ...members.filter((member) => member._id !== authUser._id && member.fullName.toLowerCase().includes(typingMention.toLowerCase())),
  ].slice(0, 6);
  const openPermissions = (member) => { setDenied(community.moderatorLimits?.find((limit) => limit.userId === member._id)?.denied || []); setPermissionsFor(member); open("permissions"); };
  const openActivity = async () => { setActivity(null); open("activity"); setActivity(await store.getActivity()); };
  const choose = async (item) => {
    await store.select(item._id);
    setShowChannels(false);
  };
  const submitDialog = async (event) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    try {
      let done = false;
      if (dialog === "create") done = await store.create(value);
      if (dialog === "channel")
        done = await store.createChannel(value, channelType);
      if (dialog === "rename-community" || dialog === "rename-channel")
        done = await store.rename(value, dialog === "rename-channel" ? renameChannel : null);
      if (dialog === "join") {
        const token = value.match(/[a-f0-9]{48}/i)?.[0];
        if (!token) {
          toast.error("Paste a valid invite link or code");
          return;
        }
        done = await store.join(token);
      }
      if (done) setDialog(null);
    } finally {
      setBusy(false);
    }
  };
  const openBans = async () => {
    setBans(null);
    open("bans");
    setBusy(true);
    try { setBans(await store.getBans()); } finally { setBusy(false); }
  };
  const generateInvite = async () => {
    open("invite");
    setBusy(true);
    try {
      const result = await store.invite();
      if (result) setInvite(result);
    } finally {
      setBusy(false);
    }
  };
  const send = async (event) => {
    event.preventDefault();
    if (!text.trim() || sending) return;
    const draft = text;
    setSending(true);
    try {
      const named = mentionsIn(draft, members);
      if (editing) {
        if (await store.edit(editing, draft.trim(), named)) { clearSent(draft); setEditing(null); }
        return;
      }
      const target = replyTo?._id;
      if (await store.send(draft.trim(), target, null, null, named)) {
        clearSent(draft);
        setReplyTo(current => current?._id === target ? null : current);
      }
    } finally {
      setSending(false);
    }
  };
  const reactions = reactionSummary(messages, now);
  const visible = messages.filter(
    (message) =>
      !message.reaction && !preferences.isBlocked(message.senderId) && (message.pinned || new Date(message.expiresAt).getTime() > now) &&
      (!pins || message.pinned),
  );
  const servers = communities.filter((item) => item.kind !== "group");
  return (
    <div
      className={`workspace community-workspace ${community ? "has-chat" : ""} ${showChannels ? "show-channels" : ""}`}
    >
      <nav className="rail" aria-label="Community navigation">
        <button
          className="brand-button"
          title="Direct messages"
          onClick={onDirectMessages}
        >
          <span className="sonar-mark" />
        </button>
        <span className="rail-rule" />
        <button
          className="rail-button"
          title="Direct messages"
          onClick={onDirectMessages}
        >
          <MessageCircle size={21} />
          {hasUnreadDirect && <span className="unread-dot rail-unread" aria-label="New direct messages" />}
        </button>
        <div className="community-rail-list">
          {servers.map((item) => (
            <button
              key={item._id}
              title={item.name}
              aria-label={`Open ${item.name}`}
              aria-pressed={item._id === community?._id}
              className={`community-icon ${item._id === community?._id ? "active" : ""}`}
              onClick={() => choose(item)}
            >
              {initials(item.name)}
              {Object.keys(unreadChannels).some(key => key.startsWith(`${item._id}:`)) && <span className="unread-dot rail-unread" aria-label="New community messages" />}
            </button>
          ))}
        </div>
        <button
          className="rail-button add-community"
          title="Create or join a community"
          onClick={() => open("start")}
        >
          <Plus size={22} />
        </button>
        <div className="rail-bottom">
          <button
            className="rail-button"
            title="Settings"
            aria-label="Settings"
            onClick={() => useAuthStore.setState({ settingsOpen: true })}
          >
            <Settings size={22} />
          </button>
        </div>
      </nav>
      <aside className="sidebar">
        <header className="space-title community-header" ref={menuRef}>
          <button className="icon-button" title="Back to messages" aria-label="Back to messages" onClick={onDirectMessages}>
            <ArrowLeft size={18} />
          </button>
          {community ? (
            <button className="community-title" aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(!menuOpen)}>
              <span className="community-name">{community.name}</span>
              <small>
                {community.members.length} {community.members.length === 1 ? "member" : "members"} · {community.members.filter((id) => onlineUsers.includes(String(id?._id || id))).length} online
              </small>
              <ChevronDown size={16} />
            </button>
          ) : (
            <span className="community-name">Communities</span>
          )}
          {community && (
            <button
              className={`icon-button ${membersOpen ? "accent" : ""}`}
              title="Show members"
              onClick={() => setMembersOpen(!membersOpen)}
            >
              <Users size={18} />
            </button>
          )}
          {community && menuOpen && (
            <div className="community-menu" role="menu" aria-label={`${community.name} options`}>
              {owner && !group && (
                <button role="menuitem" onClick={() => { setMenuOpen(false); generateInvite(); }}>
                  <UserPlus size={16} /> Invite your people
                </button>
              )}
              <button role="menuitem" onClick={() => { setMenuOpen(false); preferences.toggleMute(community._id); }}>
                {preferences.isMuted(community._id) ? <Bell size={16} /> : <BellOff size={16} />}
                {preferences.isMuted(community._id) ? `Unmute ${group ? "group" : "server"}` : `Mute ${group ? "group" : "server"}`}
              </button>
              {(owner || moderator) && !group && (
                <>
                  <hr />
                  <small className="community-menu-label">MANAGE</small>
                  <button role="menuitem" onClick={() => { setMenuOpen(false); openActivity(); }}>
                    <Users size={16} /> Who joined or left
                  </button>
                  {may("lock") && community.channels.filter((item) => item.type === "voice").map((item) => (
                    <button role="menuitem" key={item._id} onClick={async () => {
                      setMenuOpen(false);
                      if (await store.setVoiceLock(item._id, !item.locked))
                        toast(item.locked ? "Voice room unlocked." : "Voice room locked to new participants.");
                    }}>
                      {item.locked ? <LockOpen size={16} /> : <Lock size={16} />}
                      {item.locked ? "Unlock" : "Lock"} {item.name}
                    </button>
                  ))}
                  {owner && (
                    <button role="menuitem" onClick={() => { setMenuOpen(false); open("rename-community"); setValue(community.name); }}>
                      <Pencil size={16} /> Rename community
                    </button>
                  )}
                  {owner && (
                    <button role="menuitem" onClick={() => { setMenuOpen(false); open("rename-channel"); const first = community.channels[0]; setRenameChannel(first?._id || ""); setValue(first?.name || ""); }}>
                      <Pencil size={16} /> Rename a channel or room
                    </button>
                  )}
                </>
              )}
              <hr />
              {owner && !group && (
                <button role="menuitem" className="danger" onClick={() => { setMenuOpen(false); open("delete-channel"); setRenameChannel(community.channels[0]?._id || ""); }}>
                  <Trash2 size={16} /> Delete a channel or room
                </button>
              )}
              {owner && !group && (
                <button role="menuitem" className="danger" onClick={() => { setMenuOpen(false); setConfirmation({ action: "delete-community", communityId: community._id }); }}>
                  <Trash2 size={16} /> Delete community
                </button>
              )}
              {!owner && (
                <button role="menuitem" className="danger" onClick={() => { setMenuOpen(false); setConfirmation({ action: "leave", communityId: community._id }); }}>
                  <LogOut size={16} /> {group ? "Leave group" : "Leave community"}
                </button>
              )}
            </div>
          )}
        </header>
        <div className="sidebar-body">
          {community ? (
            <>
              <div className="sidebar-section">
                <span>CHANNELS</span>
                {owner && !group && (
                  <button
                    className="icon-button"
                    title="Create a channel"
                    onClick={() => {
                      setChannelType("text");
                      open("channel");
                    }}
                  >
                    <Plus size={16} />
                  </button>
                )}
              </div>
              {community.channels
                .filter((item) => item.type !== "voice")
                .map((item) => (
                  <button
                    key={item._id}
                    className={`nav-row ${item._id === channel?._id ? "selected" : ""}`}
                    onClick={() => store.selectChannel(item)}
                  >
                    <Hash size={17} />
                    {item.name}
                    {unreadChannels[`${community._id}:${item._id}`] && <span className="unread-dot" aria-label="New messages" />}
                    {item._id === channel?._id && <LockKeyhole size={11} />}
                  </button>
                ))}
              <VoiceRoomList
                community={community}
                voice={voice}
                voiceRoom={voiceRoom}
                onCreate={() => {
                  setChannelType("voice");
                  open("channel");
                }}
              />
            </>
          ) : (
            <>
              <div className="sidebar-section">
                <span>YOUR COMMUNITIES</span>
              </div>
              {servers.map((item) => (
                <button
                  className="person"
                  key={item._id}
                  onClick={() => choose(item)}
                >
                  <span className="avatar-tile">{initials(item.name)}</span>
                  <span>
                    <strong>{item.name}</strong>
                    <small>{item.members.length} members</small>
                  </span>
                  <ArrowUpRight size={15} />
                </button>
              ))}
              <button className="nav-row" onClick={() => open("create")}>
                <Plus size={17} /> Create a community
              </button>
              <button className="nav-row" onClick={() => open("join")}>
                <Users size={17} /> Join with an invite
              </button>
            </>
          )}
        </div>
        <div className="privacy-mini">
          <Timer size={19} />
          <div>
            <strong>Keep what matters.</strong>
            <small>7-day history. Pins stay.</small>
          </div>
        </div>
        <PresenceControl>
          <button className="icon-button" title="Sign out" onClick={logout}>
            <LogOut size={17} />
          </button>
        </PresenceControl>
      </aside>
      <main className="conversation">
        <header className="chat-top">
          <div className="chat-identity">
            {community && (
              <button
                className="icon-button mobile-back"
                title="Show channels"
                onClick={() => setShowChannels(true)}
              >
                <ArrowLeft size={18} />
              </button>
            )}
            <Hash size={22} />
            <div>
              <strong>{channel?.name || "Your communities"}</strong>
              <small>
                {community ? community.name : "A place for your people"}
              </small>
            </div>
          </div>
          <div className="chat-actions">
            <span className="encrypted-badge">
              <LockKeyhole size={12} />
              Encrypted
            </span>
            {channel && (
              <button className="icon-button" title="Search messages" aria-label="Search messages" onClick={() => setSearching(true)}>
                <Search size={18} />
              </button>
            )}
            {channel && (
              <button
                className={`icon-button ${pins ? "accent" : ""}`}
                title="Pinned messages"
                onClick={() => {
                  const next = !pins;
                  setPins(next);
                  store.selectChannel(channel, next);
                }}
              >
                <Pin size={18} />
              </button>
            )}
            {community && (
              <button
                className={`icon-button ${membersOpen ? "accent" : ""}`}
                title="Members"
                onClick={() => setMembersOpen(!membersOpen)}
              >
                <Users size={19} />
              </button>
            )}
          </div>
        </header>
        <div className="retention-banner">
          <Timer size={14} />
          <span>
            {channel?.disappearAfter
              ? `Disappearing messages are on. New messages vanish after ${channel.disappearAfter === 3600 ? "1 hour" : "1 day"}.`
              : "Messages disappear after 7 days. Pin the ones worth keeping."}
          </span>
          {channel && canDisappear && (
            <select aria-label="Disappearing messages" title="Disappearing messages" className="disappear-select" value={channel.disappearAfter || 0} onChange={(event) => store.setDisappearAfter(channel._id, Number(event.target.value))}>
              <option value={0}>7 days</option>
              <option value={3600}>1 hour</option>
              <option value={86400}>1 day</option>
            </select>
          )}
        </div>
        <VoiceCallBanner voice={voice} voiceRoom={voiceRoom} users={community?.memberDetails || []} watch={watch} />
        <VoiceRoomBanner voiceRoom={voiceRoom} watch={watch} />
        <div className="community-content">
          <div className="community-thread">
            <div
              className="message-scroll"
              ref={history.scroll}
              onScroll={history.onScroll}
            >
              {!community ? (
                <div className="welcome">
                  <div className="orbit">
                    <Users size={37} className="accent" />
                  </div>
                  <span className="eyebrow">
                    GOOD COMPANY. YOUR OWN CORNER.
                  </span>
                  <h1>Make room for your people.</h1>
                  <p>
                    A weekend crew. A study group. Your next big idea.
                    <br />
                    Bring everyone together in a private community.
                  </p>
                  <div className="welcome-buttons">
                    <button className="primary" onClick={() => open("create")}>
                      <Plus size={17} />
                      Create a community
                    </button>
                    <button className="secondary" onClick={() => open("join")}>
                      Join with an invite <ArrowUpRight size={16} />
                    </button>
                  </div>
                  <div className="welcome-values">
                    <span>
                      <LockKeyhole size={16} />
                      Encrypted channel messages
                    </span>
                    <span>
                      <Timer size={16} />
                      7-day history
                    </span>
                    <span>
                      <Users size={16} />
                      Invite-only spaces
                    </span>
                  </div>
                </div>
              ) : (
                <>
                  <div className="conversation-start">
                    <div className="channel-symbol">
                      <Hash size={35} />
                    </div>
                    <h1>
                      {pins
                        ? "The things worth keeping."
                        : `Welcome to #${channel?.name || "general"}`}
                    </h1>
                    <p>
                      {pins
                        ? "Pinned messages stay until someone unpins them."
                        : "Your own corner of the conversation. Make yourself at home."}
                    </p>
                    <span>
                      <LockKeyhole size={12} />
                      Encrypted separately for each member. New members start
                      with a clean history.
                    </span>
                  </div>
                  <div className="date-divider">
                    <span>
                      {pins ? "PINNED MESSAGES" : "CHANNEL CONVERSATION"}
                    </span>
                  </div>
                  {store.hasMore && (
                    <button
                      className="load-older"
                      disabled={store.loadingOlder || loading}
                      onClick={() => history.loadOlder(store.loadOlder)}
                    >
                      {store.loadingOlder
                        ? "Decrypting older messages..."
                        : pins
                          ? "Load older pinned messages"
                          : "Load older messages"}
                    </button>
                  )}
                  {loading && (
                    <p className="message-notice">
                      Opening your encrypted channel...
                    </p>
                  )}
                  {!loading && !visible.length && (
                    <p className="message-notice">
                      {pins
                        ? "No pinned messages yet."
                        : "Nothing here yet. Start with a hello."}
                    </p>
                  )}
                  {visible.map((message) => {
                    const sender = [
                      ...community.memberDetails,
                      ...(community.historyAuthors || []),
                    ].find((member) => member._id === message.senderId);
                    return (
                      <article
                        className={`message ${message.pinned ? "is-pinned" : ""}`}
                        key={message._id}
                      >
                        <Avatar user={sender || { _id: message.senderId }} />
                        <div className="message-content">
                          <div className="message-meta">
                            <strong>{sender?.fullName || "Member"}</strong>
                            {message.senderId === authUser._id && (
                              <span className="you-label">YOU</span>
                            )}
                            <time dateTime={message.createdAt}>
                              {new Date(message.createdAt).toLocaleString([], {
                                month: "short",
                                day: "numeric",
                                hour: "2-digit",
                                minute: "2-digit",
                              })}
                            </time>
                            {message.editedAt && <span className="edited-label">(edited)</span>}
                            {message.pinned && (
                              <span className="pin-label">
                                <Pin size={11} />
                                Pinned
                              </span>
                            )}
                          </div>
                          {message.replyTo && <div className="reply-reference">Replying to: {visible.find(item => item._id === message.replyTo && !item.unreadable)?.text || "Original message unavailable"}</div>}
                          {message.file ? <FileCard file={message.file} /> : <MessageText text={message.text} names={members.map((member) => member.fullName)} />}
                          {!pins && <ReactionBar message={message} reactions={reactions[message._id]} me={authUser._id} disabled={message.unreadable} onReact={reaction => store.send("", null, reaction)} />}
                        </div>
                        <button className="pin-action" title="Reply to message" aria-label="Reply to message" disabled={message.unreadable} onClick={() => setReplyTo(message)}><Reply size={14} /></button>
                        {message.senderId === authUser._id && !message.file && !message.unreadable && <button className="pin-action" title="Edit message" aria-label="Edit message" onClick={() => { setReplyTo(null); setEditing(message); setText(message.text); messageBox.current?.focus(); }}><Pencil size={14} /></button>}
                        {(message.senderId === authUser._id || may("delete")) && <button className="pin-action" title="Delete message" aria-label="Delete message"
                          onClick={() => { if (window.confirm("Delete this message for the community? Copies already saved elsewhere cannot be removed.")) store.deleteMessage(message); }}><Trash2 size={14} /></button>}
                        <button
                          className="pin-action"
                          title={
                            message.pinned ? "Unpin message" : "Pin message"
                          }
                          onClick={() => store.pin(message)}
                        >
                          <Pin size={14} />
                        </button>
                      </article>
                    );
                  })}
                </>
              )}
            </div>
            {community && channel && (
              <div className="composer-area">
                {replyTo && <div className="reply-composer"><span>Replying to: {replyTo.text}</span><button className="icon-button" aria-label="Cancel reply" onClick={() => setReplyTo(null)}><X size={16} /></button></div>}
                {editing && <div className="reply-composer"><span>Editing your message</span><button className="icon-button" aria-label="Cancel edit" onClick={() => { setEditing(null); setText(""); }}><X size={16} /></button></div>}
                {mentionOptions.length > 0 && (
                  <div className="mention-picker" role="listbox" aria-label="Mention someone">
                    {mentionOptions.map((member) => (
                      <button type="button" key={member._id} onClick={() => { setText(completeMention(text, member.fullName)); messageBox.current?.focus(); }}>
                        {member._id !== "everyone" && <Avatar user={member} />}
                        @{member.fullName}
                      </button>
                    ))}
                  </div>
                )}
                <form className="composer" onSubmit={send}>
                  <LockKeyhole size={18} />
                  <textarea
                    ref={messageBox}
                    aria-label={`Message ${channel.name}`}
                    rows={1}
                    maxLength={4000}
                    placeholder={`Message #${channel.name}...`}
                    value={text}
                    onChange={(event) => { setText(event.target.value); if (event.target.value) typed(); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        send(event);
                      }
                    }}
                  />
                  <ComposerTools
                    textarea={messageBox}
                    value={text}
                    setValue={setText}
                    target={{ community: community._id, channel: channel._id }}
                    disabled={loading}
                    stillHere={() => useCommunityStore.getState().channel?._id === channel._id}
                    send={(file) => store.send(fileLabel(file), null, null, file)}
                  />
                  <button
                    title="Send encrypted channel message"
                    disabled={sending || !text.trim() || loading}
                  >
                    <Send size={18} />
                  </button>
                </form>
                <div className="composer-hint">
                  <span>
                    {typing.length ? typingText(typing.map((person) => members.find((member) => member._id === person)?.fullName.split(" ")[0] || "Someone")) : <>
                      <LockKeyhole size={11} />
                      Encrypted for {community.members.length}{" "}
                      {community.members.length === 1 ? "member" : "members"}
                    </>}
                  </span>
                  <span>Enter to send · Shift + Enter for a new line</span>
                </div>
              </div>
            )}
          </div>
          {community && membersOpen && (
            <aside className="community-members">
              <div className="sidebar-section">
                <span>MEMBERS — {community.members.length}</span>
                <button
                  title="Close members"
                  className="icon-button"
                  onClick={() => setMembersOpen(false)}
                >
                  <X size={15} />
                </button>
              </div>
              {owner && <button className="nav-row" onClick={openBans}>Manage bans</button>}
              {community.memberDetails.map((member) => (
                <div className="person" key={member._id}>
                  <Avatar user={member} dot={onlineUsers.includes(member._id)} />
                  <span>
                    <strong>{member.fullName}</strong>
                    <small>
                      {member._id === community.ownerId ? "Owner" : community.moderators?.includes(member._id) ? "Moderator" : "Member"}
                      {member._id === authUser._id ? " · You" : ""}
                    </small>
                  </span>
                  {(() => {
                    const others = member._id !== authUser._id;
                    const manage = !group && others && (owner || (moderator && member._id !== community.ownerId && !community.moderators?.includes(member._id) && may("remove")));
                    const isModerator = community.moderators?.includes(member._id);
                    const ask = (action) => setConfirmation({ action, member, communityId: community._id });
                    return (
                      <ActionMenu label={`Options for ${member.fullName}`} items={[
                        manage && { label: "Remove from community", icon: <LogOut size={15} />, onClick: () => ask("remove") },
                        manage && { label: "Ban from community", icon: <Ban size={15} />, danger: true, onClick: () => ask("ban") },
                        manage && owner && { label: isModerator ? "Remove moderator role" : "Make moderator", icon: <ShieldCheck size={15} />, onClick: () => ask(isModerator ? "demote" : "promote") },
                        manage && owner && isModerator && { label: "Moderator permissions", icon: <Settings size={15} />, onClick: () => openPermissions(member) },
                        manage && owner && { label: "Make owner", icon: <Crown size={15} />, onClick: () => ask("transfer") },
                        "-",
                        others && { label: "Block", icon: <Ban size={15} />, danger: true, onClick: () => { if (window.confirm(`Block ${member.fullName}? Their messages will be hidden and they will not be told.`)) preferences.block(member._id); } },
                      ]} />
                    );
                  })()}
                </div>
              ))}
            </aside>
          )}
        </div>
        <footer className="workspace-footer">
          <span>
            <span className="green-dot" />
            Your space. Your people.
          </span>
          <span>Always free. Always ad-free.</span>
        </footer>
      </main>
      {searching && channel && (
        <SearchPanel search={store.searchAll} senderName={(message) => (message.senderId === authUser._id ? "You" : [...members, ...(community.historyAuthors || [])].find((member) => member._id === message.senderId)?.fullName || "Member")} onClose={() => setSearching(false)} />
      )}
      {confirmation && (
        <div
          className="modal-backdrop"
          onClick={() => !busy && setConfirmation(null)}
        >
          <section
            className="detail-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Confirm membership change"
            onClick={(event) => event.stopPropagation()}
          >
            <button
              className="modal-close icon-button"
              aria-label="Close"
              disabled={busy}
              onClick={() => setConfirmation(null)}
            >
              <X size={20} />
            </button>
            <ShieldCheck className="accent" size={29} />
            <h2>
              {confirmation.action === "delete-community"
                ? `Delete ${community?.name}?`
                : confirmation.action === "delete-channel"
                ? `Delete ${confirmation.channel.name}?`
                : confirmation.action === "leave"
                ? `Leave ${community?.name}?`
                : confirmation.action === "ban"
                ? `Ban ${confirmation.member.fullName}?`
                : confirmation.action === "unban"
                ? `Remove the ban on ${confirmation.member.fullName}?`
                : confirmation.action === "remove"
                  ? `Remove ${confirmation.member.fullName}?`
                  : confirmation.action === "promote"
                    ? `Make ${confirmation.member.fullName} a moderator?`
                    : confirmation.action === "demote"
                      ? `Remove ${confirmation.member.fullName} as moderator?`
                      : `Make ${confirmation.member.fullName} the owner?`}
            </h2>
            <p>
              {confirmation.action === "delete-community"
                ? "This permanently deletes the community, its channels and stored messages, including pins, for everyone. Active voice rooms close. Copies and backups may remain."
                : confirmation.action === "delete-channel"
                ? "This permanently deletes the channel and its stored messages, including pins. Anyone in its voice room will be disconnected. Copies and backups may remain."
                : confirmation.action === "ban"
                ? "They will lose access and cannot rejoin while banned. The current invite will be revoked. Previously received copies cannot be erased from their device."
                : confirmation.action === "unban"
                ? "They will remain outside the community. The current invite will be revoked; create a new invite if you want them to rejoin."
                : confirmation.action === "promote"
                ? "Moderators can remove or ban ordinary members. Only the owner can manage moderators, invites, channels, and ownership."
                : confirmation.action === "demote"
                ? "They will remain a member but will no longer be able to remove members."
                : confirmation.action === "transfer"
                ? "They will control invites, channels, and membership. You will remain a member and lose owner controls. The current invite will be revoked."
                : confirmation.action === "remove"
                  ? "They will lose access to this community and future messages. The current invite will be revoked so it cannot be used to rejoin. Previously received copies cannot be erased from their device."
                  : "You will lose access to channels and future messages. You will need a valid invite to rejoin. Messages you sent remain subject to the community retention rules."}
            </p>
            <div className="dialog-actions">
              <button
                className={
                  ["transfer", "promote"].includes(confirmation.action)
                    ? "primary"
                    : "danger-button"
                }
                disabled={busy}
                onClick={confirmMemberAction}
              >
                {busy
                  ? "Updating membership..."
                  : confirmation.action === "delete-community"
                    ? "Delete community permanently"
                    : confirmation.action === "delete-channel"
                    ? "Delete channel"
                    : confirmation.action === "ban"
                    ? "Ban member"
                    : confirmation.action === "unban"
                    ? "Remove ban"
                    : confirmation.action === "promote"
                    ? "Make moderator"
                    : confirmation.action === "demote"
                    ? "Remove moderator role"
                    : confirmation.action === "transfer"
                    ? "Transfer ownership"
                    : confirmation.action === "remove"
                      ? "Remove member"
                      : "Leave community"}
              </button>
              <button
                className="secondary"
                disabled={busy}
                onClick={() => setConfirmation(null)}
              >
                Cancel
              </button>
            </div>
          </section>
        </div>
      )}
      {dialog && (
        <div
          className="modal-backdrop"
          onClick={() => !busy && setDialog(null)}
        >
          <section
            className="detail-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Community actions"
            onClick={(event) => event.stopPropagation()}
          >
            <button
              className="modal-close icon-button"
              title="Close"
              disabled={busy}
              onClick={() => setDialog(null)}
            >
              <X size={20} />
            </button>
            <Users size={29} className="accent" />
            {dialog === "start" ? (
              <>
                <h2>Your people belong here.</h2>
                <p>
                  Start a private community, or join a space with an invite from
                  its owner.
                </p>
                <div className="dialog-actions">
                  <button className="primary" onClick={() => open("create")}>
                    <Plus size={17} />
                    Create a community
                  </button>
                  <button className="secondary" onClick={() => open("join")}>
                    Join with an invite
                    <ArrowUpRight size={17} />
                  </button>
                </div>
              </>
            ) : dialog === "invite" ? (
              <>
                <h2>Good company starts here.</h2>
                <p>
                  This invite expires in 24 hours. Generating a new invite
                  revokes the previous link. Anyone with it can join, up to 32
                  members.
                </p>
                {invite ? (
                  <>
                    <label className="dialog-field">
                      Invite code
                      <input
                        readOnly
                        value={invite.token}
                        onFocus={(event) => event.target.select()}
                      />
                    </label>
                    <button
                      className="primary"
                      onClick={async () => {
                        try {
                          await navigator.clipboard.writeText(
                            `${window.location.origin}/?invite=${invite.token}`,
                          );
                          toast.success("Invite link copied");
                        } catch {
                          toast.error("Select and copy the invite code above");
                        }
                      }}
                    >
                      <Copy size={16} />
                      Copy invite link
                    </button>
                    <div className="honest-note">
                      <Timer size={16} />
                      <p>
                        New members receive future messages only. Their devices
                        cannot decrypt older history.
                      </p>
                    </div>
                  </>
                ) : (
                  <p className="message-notice">
                    {busy
                      ? "Creating your invite..."
                      : "Unable to create an invite. Close and try again."}
                  </p>
                )}
              </>
            ) : dialog === "activity" ? (
              <>
                <h2>Who joined or left</h2>
                <p>The latest 50 changes to this community&apos;s members.</p>
                {!activity ? <p>Loading...</p> : !activity.length ? <p>Nothing yet.</p> :
                  <ul className="session-list">{activity.map((row, index) => <li key={index}><strong>{row.name}</strong> {row.action} <small>{new Date(row.at).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</small></li>)}</ul>}
              </>
            ) : dialog === "permissions" ? (
              <>
                <h2>What {permissionsFor?.fullName} can do</h2>
                <p>Moderators can do all of this until you switch something off.</p>
                {PERMISSIONS.map(([value, label]) => (
                  <label className="settings-checkbox" key={value}>
                    <input type="checkbox" checked={!denied.includes(value)} onChange={(event) => setDenied(event.target.checked ? denied.filter((item) => item !== value) : [...denied, value])} />
                    {label}
                  </label>
                ))}
                <button className="primary" disabled={busy} onClick={async () => { setBusy(true); try { if (await store.setModeratorLimits(permissionsFor._id, denied)) setDialog(null); } finally { setBusy(false); } }}>Save permissions</button>
              </>
            ) : dialog === "bans" ? (
              <>
                <h2>Community bans</h2>
                <p>Banned accounts cannot use an invite to rejoin. Removing a ban revokes the current invite.</p>
                {busy ? <p>Loading bans...</p> : !bans ? <p>Could not load bans. Close and try again.</p> : !bans.length ? <p>No banned accounts.</p> :
                  <ul className="session-list">{bans.map(member => <li key={member._id}><strong>{member.fullName}</strong><button className="secondary" onClick={() => { setDialog(null); setConfirmation({ action: "unban", member, communityId: community._id }); }}>Remove ban</button></li>)}</ul>}
              </>
            ) : dialog === "delete-channel" ? (
              <>
                <h2>Delete a channel or room</h2>
                <p>Select a channel. Its stored messages, including pins, will be deleted. Every community must keep at least one text channel.</p>
                <label className="dialog-field">Channel or room
                  <select value={renameChannel} onChange={event => setRenameChannel(event.target.value)}>
                    {community.channels.map(item => <option key={item._id} value={item._id}>{item.name} ({item.type === "voice" ? "voice" : "text"})</option>)}
                  </select>
                </label>
                <button className="danger-button" disabled={!renameChannel} onClick={() => {
                  const channel = community.channels.find(item => item._id === renameChannel);
                  if (channel) { setDialog(null); setConfirmation({ action: "delete-channel", communityId: community._id, channel }); }
                }}>Continue</button>
              </>
            ) : dialog === "rename-community" || dialog === "rename-channel" ? (
              <>
                <h2>{dialog === "rename-community" ? "Rename community" : "Rename channel or room"}</h2>
                <p>Everyone in the community will see the updated name.</p>
                <form onSubmit={submitDialog}>
                  {dialog === "rename-channel" && <label className="dialog-field">Channel or room
                    <select value={renameChannel} onChange={event => {
                      const next = community.channels.find(item => item._id === event.target.value);
                      setRenameChannel(event.target.value); setValue(next?.name || "");
                    }}>
                      {community.channels.map(item => <option key={item._id} value={item._id}>{item.name} ({item.type === "voice" ? "voice" : "text"})</option>)}
                    </select>
                  </label>}
                  <label className="dialog-field">New name
                    <input autoFocus required maxLength={dialog === "rename-community" ? 60 : 40} value={value} onChange={event => setValue(event.target.value)} />
                  </label>
                  <button className="primary" disabled={busy || !value.trim() || (dialog === "rename-channel" && !renameChannel)}>{busy ? "Saving..." : "Save name"}</button>
                </form>
              </>
            ) : (
              <>
                <h2>
                  {dialog === "create"
                    ? "A new corner of the internet."
                    : dialog === "channel"
                      ? "Give the conversation a home."
                      : "Your people are waiting."}
                </h2>
                <p>
                  {dialog === "create"
                    ? "Give your community a name. We will start you off with a general channel."
                    : dialog === "channel"
                      ? channelType === "voice"
                        ? "All community members can join. Up to six people can share encrypted audio at a time."
                        : "Only the owner can create channels. All community members can read future messages."
                      : "Paste an invite link or code from a community owner."}
                </p>
                <form onSubmit={submitDialog}>
                  {dialog === "channel" && (
                    <label className="dialog-field">
                      Channel type
                      <select
                        aria-label="Channel type"
                        value={channelType}
                        onChange={(event) => setChannelType(event.target.value)}
                      >
                        <option value="text">Text channel</option>
                        <option value="voice">
                          Voice room · up to 6 people
                        </option>
                      </select>
                    </label>
                  )}
                  <label className="dialog-field">
                    {dialog === "join"
                      ? "Invite link or code"
                      : dialog === "channel"
                        ? channelType === "voice"
                          ? "Room name"
                          : "Channel name"
                        : "Community name"}
                    <input
                      autoFocus
                      required
                      maxLength={
                        dialog === "join"
                          ? 2048
                          : dialog === "channel"
                            ? 40
                            : 60
                      }
                      value={value}
                      placeholder={
                        dialog === "create"
                          ? "The weekend crew"
                          : dialog === "channel"
                            ? channelType === "voice"
                              ? "Lounge"
                              : "Ideas and plans"
                            : "Paste your invite here"
                      }
                      onChange={(event) => setValue(event.target.value)}
                    />
                  </label>
                  <button className="primary" disabled={busy || !value.trim()}>
                    {busy
                      ? "One moment..."
                      : dialog === "join"
                        ? "Join community"
                        : dialog === "channel"
                          ? channelType === "voice"
                            ? "Create voice room"
                            : "Create channel"
                          : "Create community"}
                    <Check size={16} />
                  </button>
                </form>
                <div className="honest-note">
                  <ShieldCheck size={16} />
                  <p>
                    Messages are encrypted on your device. Community names,
                    channel names, and membership are visible to the service.
                    This early encryption protocol has not been independently
                    audited.
                  </p>
                </div>
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
