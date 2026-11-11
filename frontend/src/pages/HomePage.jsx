import { presenceLabel } from "../lib/presence";
import PresenceControl from "../components/PresenceControl";
import { useEffect, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
  ChevronDown,
  ChevronLeft,
  Headphones,
  LockKeyhole,
  LogOut,
  MessageCircle,
  MoreHorizontal,
  Phone,
  Pin,
  Search,
  Reply,
  Pencil,
  Plus,
  Ban,
  Send,
  Settings,
  ShieldCheck,
  Timer,
  Trash2,
  UserMinus,
  Users,
  X,
} from "lucide-react";
import { useAuthStore } from "../store/useAuthStore";
import { useChatStore } from "../store/useChatStore";
import { useCommunityStore } from "../store/useCommunityStore";
import { useUnreadStore } from "../store/useUnreadStore";
import { useVoiceCall } from "../lib/useVoiceCall";
import { useVoiceRoom } from "../lib/useVoiceRoom";
import { useWatch } from "../lib/useWatch";
import VoiceRoomBanner from "../components/VoiceRoomBanner";
import { decrypt, fingerprint, contactKeyStatus, acceptVerifiedContactKey } from "../lib/crypto";
import VoiceCallBanner from "../components/VoiceCallBanner";
import ReactionBar from "../components/ReactionBar";
import MessageText from "../components/MessageText";
import CallNotice from "../components/CallNotice";
import Avatar from "../components/Avatar";
import ActionMenu from "../components/ActionMenu";
import SearchPanel from "../components/SearchPanel";
import { usePreferencesStore, matchesKeyword } from "../store/usePreferencesStore";
import { useTyping, typingText } from "../lib/useTyping";
import { mentionsMe } from "../lib/mentions";
import toast from "react-hot-toast";
import { messageAlert } from "../lib/messageAlert";
import ComposerTools, { FileCard } from "../components/ComposerTools";
import { fileLabel } from "../lib/messageContent";
import { reactionSummary } from "../lib/reactions";
import CommunityWorkspace from "../components/CommunityWorkspace";
import ExperimentalContactVerification from "../components/ExperimentalContactVerification";
import ExperimentalMatrixChat from "../components/ExperimentalMatrixChat";
import { useConversationDraft } from "../store/useDraftStore";
import { useHistoryScroll } from "../lib/useHistoryScroll";
function FriendNote({ me, person }) {
  const [text, setText] = useState("");
  useEffect(() => {
    let active = true;
    decrypt(me, person, { ...person.note, senderId: person._id, receiverId: me, channelId: "000000000000000000000001" }).then((value) => { if (active && !value.unreadable) setText(value.text); });
    return () => { active = false; };
  }, [me, person]);
  return text ? <em className="friend-note">{`"${text}"`}</em> : null;
}
const matches = (user, text) => {
  const value = text.trim().replace(/^@/, "").toLowerCase();
  return user.fullName.toLowerCase().includes(value) || (user.username || "").includes(value);
};
export default function HomePage() {
  const [communityMode, setCommunityMode] = useState(
    () =>
      new URLSearchParams(window.location.search).has("invite") ||
      !!sessionStorage.getItem("sonar-invite"),
  );
  const { authUser, onlineUsers, presence, logout, socket } = useAuthStore();
  const statusText = (id, here) => (!onlineUsers.includes(id) ? "Offline" : presence[id] ? presenceLabel(presence[id]) : here);
  const {
    users,
    messages,
    selectedUser: peer,
    setSelectedUser,
    getUsers,
    friendRequests,
    getMessages,
    sendMessage,
    pinMessage,
    deleteMessage,
    subscribeToMessages,
    unsubscribeFromMessages,
    isUsersLoading,
    isMessagesLoading,
    hasMore,
    loadingOlder,
    loadOlder,
  } = useChatStore();
  const messageBox = useRef(null);
  const [query, setQuery] = useState(""),
    [peopleQuery, setPeopleQuery] = useState(""),
    [friendNote, setFriendNote] = useState(""),
    [editing, setEditing] = useState(null),
    [groupPicks, setGroupPicks] = useState([]),
    [groupName, setGroupName] = useState(""),
    [sending, setSending] = useState(false),
    [panel, setPanel] = useState(null),
    [onlyPins, setOnlyPins] = useState(false),
    [now, setNow] = useState(Date.now()),
    [print, setPrint] = useState(""),
    [contactTrust, setContactTrust] = useState(null),
    [trustNotice, setTrustNotice] = useState(""),
    [verifyingContact, setVerifyingContact] = useState(false),
    [experimentalPane, setExperimentalPane] = useState("verify"),
    [replyTo, setReplyTo] = useState(null);
  const { text, setText, clearSent } = useConversationDraft(
    peer ? `dm:${peer._id}` : null,
  );
  const preferences = usePreferencesStore();
  const groups = useCommunityStore((state) => state.communities).filter((item) => item.kind === "group");
  const { typing, typed } = useTyping(socket, peer ? { to: peer._id } : null);
  const unread = useUnreadStore(state => state.direct);
  const hasUnread = Object.keys(unread).length > 0;
  const unreadChannels = useUnreadStore(state => state.channels);
  const hasUnreadChannels = Object.keys(unreadChannels).length > 0;
  const history = useHistoryScroll(messages, `dm:${peer?._id}:${onlyPins}`);
  const voiceRoom = useVoiceRoom(socket);
  const voice = useVoiceCall(socket, Boolean(voiceRoom.room));
  // Watch Together runs inside an answered call or a joined voice room.
  const watch = useWatch(socket, voiceRoom.room?.status === "joined" || ["connected", "reconnecting"].includes(voice.call?.status));
  useEffect(() => {
    const prune = () => {
      const now = Date.now();
      useChatStore.getState().pruneExpired(now);
      useCommunityStore.getState().pruneExpired(now);
    };
    prune();
    const timer = setInterval(prune, 15000);
    document.addEventListener("visibilitychange", prune);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", prune); };
  }, []);
  useEffect(() => {
    const { getFriendRequests } = useChatStore.getState();
    // Someone with nobody yet is taken straight to where they can add people.
    Promise.all([getUsers(), getFriendRequests()]).then(() => {
      if (!useChatStore.getState().users.length) setPanel("people");
    });
    useCommunityStore.getState().load();
    usePreferencesStore.getState().load();
    const timer = setInterval(() => {
      getUsers();
      getFriendRequests();
      useCommunityStore.getState().load();
      setNow(Date.now());
    }, 30000);
    return () => clearInterval(timer);
  }, [getUsers]);
  const unreadTotal = Object.keys(unread).length + Object.keys(unreadChannels).length;
  useEffect(() => {
    document.title = unreadTotal ? `(${unreadTotal}) Sonar` : "Sonar";
    return () => { document.title = "Sonar"; };
  }, [unreadTotal]);
  useEffect(() => {
    if (!socket) return;
    const refresh = () => { getUsers(); useChatStore.getState().getFriendRequests(); };
    const asked = ({ fullName }) => toast(`${fullName} sent you a friend request`);
    const reloadPreferences = () => usePreferencesStore.getState().load();
    const animalChanged = ({ userId, animal }) => {
      const patch = (person) => (person?._id === userId ? { ...person, animal } : person);
      const chat = useChatStore.getState(), community = useCommunityStore.getState().community;
      useChatStore.setState({ users: chat.users.map(patch), selectedUser: patch(chat.selectedUser) });
      if (community) useCommunityStore.setState({ community: { ...community, memberDetails: community.memberDetails.map(patch), historyAuthors: (community.historyAuthors || []).map(patch) } });
    };
    socket.on("friendsChanged", refresh);
    socket.on("friendRequest", asked);
    socket.on("preferencesChanged", reloadPreferences);
    socket.on("profileChanged", animalChanged);
    return () => { socket.off("friendsChanged", refresh); socket.off("friendRequest", asked); socket.off("preferencesChanged", reloadPreferences); socket.off("profileChanged", animalChanged); };
  }, [socket, getUsers]);
  useEffect(() => {
    const update = () => {
      useUnreadStore.getState().setActivePeer(!communityMode && !onlyPins && !document.hidden ? peer?._id || null : null);
      useChatStore.getState().recordVisibleDirect();
    };
    update();
    document.addEventListener("visibilitychange", update);
    return () => { document.removeEventListener("visibilitychange", update); useUnreadStore.getState().setActivePeer(null); };
  }, [communityMode, peer?._id, onlyPins]);
  useEffect(() => {
    setOnlyPins(false);
    setReplyTo(null);
    setEditing(null);
  }, [peer?._id]);
  useEffect(() => {
    setReplyTo(current => current && !messages.some(message => message._id === current._id) ? null : current);
  }, [messages]);
  useEffect(() => {
    if (peer) getMessages(peer._id, onlyPins);
  }, [peer?._id, onlyPins, getMessages]);
  useEffect(() => {
    subscribeToMessages();
    return unsubscribeFromMessages;
  }, [socket, subscribeToMessages, unsubscribeFromMessages]);
  useEffect(() => {
    if (!socket) return;
    const newChannelMessage = async (message) => {
      const me = useAuthStore.getState().authUser?._id;
      if (message.senderId === me) return;
      const settings = usePreferencesStore.getState();
      if (settings.isBlocked(message.senderId)) return;
      if (settings.isMuted(message.communityId)) {
        // A muted server stays silent unless you were named or a keyword of yours appears.
        const opened = await useCommunityStore.getState().openForAlert(message);
        if (!opened || !(mentionsMe(opened.decoded, me, opened.canEveryone) || matchesKeyword(opened.decoded.text, settings.keywords))) return;
      }
      const key = `${message.communityId}:${message.channelId}`;
      useUnreadStore.getState().markChannel(key);
      if (useUnreadStore.getState().activeChannel !== key) {
        const community = useCommunityStore.getState().communities.find((item) => item._id === message.communityId);
        messageAlert(`New message in ${community?.name || "a server"}`);
      }
    };
    const removedCommunity = ({ communityId }) => useUnreadStore.getState().clearCommunity(communityId);
    socket.on("channelMessageNew", newChannelMessage);
    socket.on("communityRemoved", removedCommunity);
    return () => { socket.off("channelMessageNew", newChannelMessage); socket.off("communityRemoved", removedCommunity); };
  }, [socket]);
  useEffect(() => {
    let active = true;
    setPrint("");
    setContactTrust(null);
    setTrustNotice("");
    if (peer?.encryptionKey)
      fingerprint(peer.encryptionKey).then((value) => {
        if (active) setPrint(value);
      }).catch(() => { if (active) setTrustNotice("Contact key is invalid."); });
    if (authUser?._id && peer?._id && peer?.encryptionKey)
      contactKeyStatus(authUser._id, peer).then((value) => {
        if (active) setContactTrust(value);
      }).catch(() => { if (active) setTrustNotice("Could not check the stored contact key."); });
    return () => {
      active = false;
    };
  }, [authUser?._id, peer?._id, peer?.encryptionKey]);
  const verifyContactChange = async () => {
    if (!peer || !contactTrust?.changed || verifyingContact) return;
    const currentPeer = peer;
    setVerifyingContact(true);
    setTrustNotice("");
    try {
      const latest = useChatStore.getState().selectedUser;
      if (latest?._id !== currentPeer._id || latest?.encryptionKey !== currentPeer.encryptionKey || useAuthStore.getState().authUser?._id !== authUser._id)
        throw new Error("Contact changed. Compare the safety code again.");
      await acceptVerifiedContactKey(authUser._id, currentPeer, contactTrust.saved);
      const active = useChatStore.getState().selectedUser;
      if (active?._id === currentPeer._id && active.encryptionKey === currentPeer.encryptionKey && useAuthStore.getState().authUser?._id === authUser._id) {
        setContactTrust({ saved: currentPeer.encryptionKey, changed: false });
        setTrustNotice("New key trusted on this device. Reload the conversation to refresh its history.");
      }
    } catch (error) { setTrustNotice(error.message); }
    finally { setVerifyingContact(false); }
  };
  const submit = async (e) => {
    e.preventDefault();
    if (!text.trim() || sending) return;
    setSending(true);
    const draft = text;
    if (editing) {
      if (await useChatStore.getState().editMessage(editing, draft.trim())) { clearSent(draft); setEditing(null); }
      setSending(false);
      return;
    }
    const target = replyTo?._id;
    if (await sendMessage({ text: draft.trim(), replyTo: target })) {
      clearSent(draft);
      setReplyTo(current => current?._id === target ? null : current);
    }
    setSending(false);
  };
  const reactions = reactionSummary(messages, now);
  const visible = messages.filter(
    (m) =>
      !m.reaction && (m.pinned || new Date(m.expiresAt).getTime() > now) &&
      (!onlyPins || m.pinned),
  );
  if (communityMode)
    return (
      <CommunityWorkspace
        onDirectMessages={() => setCommunityMode(false)}
        voice={voice}
        voiceRoom={voiceRoom}
        watch={watch}
        hasUnreadDirect={hasUnread}
      />
    );
  return (
    <div className={`workspace ${peer ? "has-chat" : ""}`}>
      <nav className="rail" aria-label="Main navigation">
        <button
          className="brand-button"
          title="Sonar home"
          onClick={() => setSelectedUser(null)}
        >
          <span className="sonar-mark" />
        </button>
        <span className="rail-rule" />
        <button
          className="rail-button active"
          title="Direct messages"
          onClick={() => setSelectedUser(null)}
        >
          <MessageCircle size={22} />
        </button>
        <button
          className="rail-button"
          title="Communities"
          onClick={() => setCommunityMode(true)}
        >
          <Users size={22} />
          {hasUnreadChannels && <span className="unread-dot rail-unread" aria-label="New community messages" />}
        </button>
        <button
          className="rail-button"
          title="Privacy details"
          onClick={() => setPanel("privacy")}
        >
          <ShieldCheck size={22} />
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
        <header className="space-title">
          <span>
            sonar
            <span className="tiny-dot" />
          </span>
          <button
            className="icon-button"
            title="About your space"
            onClick={() => setPanel("privacy")}
          >
            <ChevronDown size={18} />
          </button>
        </header>
        <div className="sidebar-body">
          <div className="space-label">YOUR SPACE</div>
          <button
            className="nav-row selected"
            onClick={() => {
              setOnlyPins(false);
              setPanel(null);
            }}
          >
            <MessageCircle size={18} /> Direct messages{" "}
            <span>{users.length}</span>
          </button>
          <button className="nav-row" onClick={() => setPanel("people")}>
            <Users size={18} /> People {friendRequests.incoming.length > 0 ? <span>{friendRequests.incoming.length}</span> : <ArrowUpRight size={15} />}
          </button>
          <div className="sidebar-section">
            <span>CONVERSATIONS</span>
            <button
              title="Find people"
              className="icon-button"
              onClick={() => { setPeopleQuery(""); setPanel("people"); }}
            >
              <Search size={15} />
            </button>
          </div>
          <label className="search-box">
            <Search size={15} />
            <input
              aria-label="Search people"
              placeholder="Find a conversation"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <kbd>/</kbd>
          </label>
          <div className="people-list">
            {isUsersLoading && !users.length && (
              <p className="muted small">Loading people...</p>
            )}
            {[
              { title: preferences.pinnedPeople.length ? "PINNED" : null, people: users.filter((u) => preferences.pinnedPeople.includes(u._id)) },
              ...preferences.folders.map((folder) => ({ title: folder.name.toUpperCase(), people: users.filter((u) => folder.people.includes(u._id) && !preferences.pinnedPeople.includes(u._id)) })),
              { title: preferences.pinnedPeople.length || preferences.folders.length ? "EVERYONE ELSE" : null, people: users.filter((u) => !preferences.pinnedPeople.includes(u._id) && !preferences.folders.some((folder) => folder.people.includes(u._id))) },
            ].map((section, index) => {
              const shown = section.people.filter((u) => matches(u, query));
              return shown.length > 0 && (
                <div key={section.title || index}>
                  {section.title && <div className="folder-label">{section.title}</div>}
                  {shown.map((user) => (
                    <button
                      key={user._id}
                      className={`person ${peer?._id === user._id ? "chosen" : ""}`}
                      onClick={() => setSelectedUser(user)}
                    >
                      <Avatar user={user} dot={onlineUsers.includes(user._id)} />
                      <span>
                        <strong>{user.fullName}</strong>
                        <small>
                          {statusText(user._id, "Around now")}
                        </small>
                      </span>
                      {unread[user._id] && <span className="unread-dot" aria-label="New messages" title="New messages" />}
                      {user.encryptionKey && (
                        <LockKeyhole size={12} className="person-lock" />
                      )}
                    </button>
                  ))}
                </div>
              );
            })}
            {!isUsersLoading && !users.length && (
              <p className="empty-people">
                Your space starts with a hello. Go to People and add someone by
                their username.
              </p>
            )}
          </div>
          <div className="sidebar-section">
            <span>GROUPS</span>
            <button title="New group" aria-label="New group" className="icon-button" onClick={() => { setGroupPicks([]); setGroupName(""); setPanel("group"); }}>
              <Plus size={15} />
            </button>
          </div>
          {groups.map((group) => (
            <button key={group._id} className="person" onClick={async () => { await useCommunityStore.getState().select(group._id); setCommunityMode(true); }}>
              <span className="avatar-tile"><Users size={18} /></span>
              <span><strong>{group.name}</strong><small>{group.members.length} people</small></span>
              {Object.keys(unreadChannels).some((key) => key.startsWith(`${group._id}:`)) && <span className="unread-dot" aria-label="New messages" />}
            </button>
          ))}
        </div>
        <div className="privacy-mini">
          <ShieldCheck size={19} />
          <div>
            <strong>A little more private.</strong>
            <small>Messages disappear after 7 days.</small>
          </div>
        </div>
        <PresenceControl>
          <button title="Sign out" className="icon-button" onClick={logout}>
            <LogOut size={17} />
          </button>
        </PresenceControl>
      </aside>
      <main className="conversation">
        <header className="chat-top">
          <div className="chat-identity">
            {peer && (
              <button
                className="icon-button mobile-back"
                aria-label="Back to conversations"
                onClick={() => setSelectedUser(null)}
              >
                <ChevronLeft />
              </button>
            )}
            {peer ? (
              <>
                <Avatar user={peer} dot={onlineUsers.includes(peer._id)} />
                <div>
                  <strong>{peer.fullName}</strong>
                  <small>
                    <span
                      className={
                        onlineUsers.includes(peer._id)
                          ? presence[peer._id] ? `status-dot status-${presence[peer._id]}` : "green-dot"
                          : "offline-dot"
                      }
                    />
                    {statusText(peer._id, "Online")}
                    <span className="header-separator">/</span> Direct message
                  </small>
                </div>
              </>
            ) : (
              <>
                <MessageCircle size={22} />
                <strong>Direct messages</strong>
              </>
            )}
          </div>
          <div className="chat-actions">
            <span className="encrypted-badge">
              <LockKeyhole size={12} />
              Encrypted
            </span>
            {peer && (
              <>
                <button
                  title={onlineUsers.includes(peer._id) ? "Start voice call" : `${peer.fullName.split(" ")[0]} is offline. Calls need both people online.`}
                  className="icon-button"
                  disabled={
                    !onlineUsers.includes(peer._id) ||
                    !!voice.call ||
                    !!voiceRoom.room
                  }
                  onClick={() => { if (!preferences.confirmCalls || window.confirm(`Call ${peer.fullName}?`)) voice.start(peer); }}
                >
                  <Phone size={19} />
                </button>
                <button title="Search messages" aria-label="Search messages" className="icon-button" onClick={() => setPanel("search")}>
                  <Search size={19} />
                </button>
                <button
                  title="Pinned messages"
                  className={`icon-button ${onlyPins ? "accent" : ""}`}
                  onClick={() => setOnlyPins(!onlyPins)}
                >
                  <Pin size={19} />
                </button>
              </>
            )}
            <button
              title="Conversation details"
              className="icon-button"
              onClick={() => setPanel("privacy")}
            >
              <MoreHorizontal size={21} />
            </button>
          </div>
        </header>
        <div className="retention-banner">
          <Timer size={14} />
          <span>
            {peer?.disappearAfter
              ? `Disappearing messages are on. New messages vanish after ${peer.disappearAfter === 3600 ? "1 hour" : "1 day"}.`
              : "A conversation, not a permanent record. Messages disappear after 7 days."}
          </span>
          {peer && (
            <select aria-label="Disappearing messages" title="Disappearing messages" className="disappear-select" value={peer.disappearAfter || 0} onChange={(event) => useChatStore.getState().setDisappearAfter(peer._id, Number(event.target.value))}>
              <option value={0}>7 days</option>
              <option value={3600}>1 hour</option>
              <option value={86400}>1 day</option>
            </select>
          )}
          <button onClick={() => setPanel("privacy")}>
            How it works <ArrowUpRight size={12} />
          </button>
        </div>
        <VoiceCallBanner voice={voice} users={users} voiceRoom={voiceRoom} watch={watch} />
        <VoiceRoomBanner voiceRoom={voiceRoom} watch={watch} />
        <div
          className="message-scroll"
          ref={history.scroll}
          onScroll={history.onScroll}
        >
          {!peer ? (
            <div className="welcome">
              <div className="orbit">
                <span className="sonar-mark" />
              </div>
              <span className="eyebrow">TUNE INTO YOUR PEOPLE</span>
              <h1>A space to just be.</h1>
              <p>
                Big ideas. Small talk. Late-night catch-ups.
                <br />
                Your next conversation starts with a hello.
              </p>
              <button className="primary" onClick={() => setPanel("people")}>
                Find your people <ArrowUpRight size={17} />
              </button>
              <div className="welcome-values">
                <span>
                  <LockKeyhole size={16} />
                  Encrypted messages
                </span>
                <span>
                  <Timer size={16} />
                  7-day message history
                </span>
                <span>
                  <Headphones size={16} />
                  Unrecorded voice
                </span>
              </div>
            </div>
          ) : (
            <>
              <div className="conversation-start">
                <Avatar user={peer} large />
                <h1>
                  {onlyPins
                    ? "The things worth keeping."
                    : `This is your space with ${peer.fullName.split(" ")[0]}.`}
                </h1>
                <p>
                  {onlyPins
                    ? "Pinned messages stay until you unpin them."
                    : "Say something. Make a plan. Stay a while."}
                </p>
                <span>
                  <LockKeyhole size={12} />
                  {peer.encryptionKey
                    ? "Encrypted on your device before sending"
                    : `${peer.fullName.split(" ")[0]} hasn't set up encryption yet. They need to sign in to Sonar once before you can message them.`}
                </span>
              </div>
              <div className="date-divider">
                <span>
                  {onlyPins ? "PINNED MESSAGES" : "YOUR CONVERSATION"}
                </span>
              </div>
              {hasMore && (
                <button
                  className="load-older"
                  disabled={loadingOlder || isMessagesLoading}
                  onClick={() => history.loadOlder(loadOlder)}
                >
                  {loadingOlder
                    ? "Decrypting older messages..."
                    : onlyPins
                      ? "Load older pinned messages"
                      : "Load older messages"}
                </button>
              )}
              {isMessagesLoading ? (
                <p className="message-notice">
                  Decrypting your conversation...
                </p>
              ) : !visible.length ? (
                <p className="message-notice">
                  {onlyPins
                    ? "No pinned messages yet."
                    : "A clean slate. Send the first hello."}
                </p>
              ) : (
                visible.map((message) => {
                  const sender =
                    message.senderId === authUser._id ? authUser : peer;
                  return (
                    <article
                      className={`message ${message.pinned ? "is-pinned" : ""}`}
                      key={message._id}
                    >
                      <Avatar user={sender} />
                      <div className="message-content">
                        <div className="message-meta">
                          <strong>{sender.fullName}</strong>
                          {sender._id === authUser._id && (
                            <span className="you-label">YOU</span>
                          )}
                          <time dateTime={message.createdAt}>
                            {new Date(message.createdAt).toLocaleTimeString(
                              [],
                              { hour: "2-digit", minute: "2-digit" },
                            )}
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
                        {message.file ? <FileCard file={message.file} /> : message.call === "missed" ? <CallNotice mine={sender._id === authUser._id} name={sender.fullName} /> : <MessageText text={message.text} onOpenCommunity={() => setCommunityMode(true)} />}
                        {!onlyPins && <ReactionBar message={message} reactions={reactions[message._id]} me={authUser._id} disabled={message.unreadable} onReact={reaction => sendMessage({ reaction })} />}
                      </div>
                      <button className="pin-action" title="Reply to message" aria-label="Reply to message" disabled={message.unreadable} onClick={() => setReplyTo(message)}><Reply size={14} /></button>
                      {message.senderId === authUser._id && !message.file && !message.call && !message.unreadable && (
                        <button className="pin-action" title="Edit message" aria-label="Edit message" onClick={() => { setReplyTo(null); setEditing(message); setText(message.text); messageBox.current?.focus(); }}><Pencil size={14} /></button>
                      )}
                      {message.senderId === authUser._id && (
                        <button className="pin-action" title="Delete message" aria-label="Delete message"
                          onClick={() => {
                            if (window.confirm("Delete this message for both people? Copies already saved elsewhere cannot be removed.")) deleteMessage(message);
                          }}><Trash2 size={14} /></button>
                      )}
                      <button
                        className="pin-action"
                        title={
                          message.pinned ? "Unpin message" : "Keep this message"
                        }
                        onClick={() => pinMessage(message)}
                      >
                        <Pin size={14} />
                      </button>
                    </article>
                  );
                })
              )}
            </>
          )}
        </div>
        {peer && (
          <div className="composer-area">
            {replyTo && <div className="reply-composer"><span>Replying to: {replyTo.text}</span><button className="icon-button" aria-label="Cancel reply" onClick={() => setReplyTo(null)}><X size={16} /></button></div>}
            {editing && <div className="reply-composer"><span>Editing your message</span><button className="icon-button" aria-label="Cancel edit" onClick={() => { setEditing(null); setText(""); }}><X size={16} /></button></div>}
            <form className="composer" onSubmit={submit}>
              <LockKeyhole size={18} />
              <textarea
                ref={messageBox}
                aria-label="Message"
                rows={1}
                maxLength={4000}
                placeholder={peer.encryptionKey ? `Message ${peer.fullName.split(" ")[0]}...` : `${peer.fullName.split(" ")[0]} needs to sign in to Sonar first`}
                value={text}
                disabled={!peer.encryptionKey}
                onChange={(e) => { setText(e.target.value); if (e.target.value) typed(); }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    submit(e);
                  }
                }}
              />
              <ComposerTools
                textarea={messageBox}
                value={text}
                setValue={setText}
                target={{ to: peer._id }}
                disabled={!peer.encryptionKey}
                stillHere={() => useChatStore.getState().selectedUser?._id === peer._id}
                send={(file) => sendMessage({ text: fileLabel(file), file })}
              />
              <button
                title="Send encrypted message"
                disabled={sending || !text.trim() || !peer.encryptionKey}
              >
                <Send size={18} />
              </button>
            </form>
            <div className="composer-hint">
              <span>
                {typing.length ? typingText([peer.fullName.split(" ")[0]]) : <><LockKeyhole size={11} /> Only your devices hold the message keys</>}
              </span>
              <span>
                Enter to send <span>·</span> Shift + Enter for a new line
              </span>
            </div>
          </div>
        )}
        <footer className="workspace-footer">
          <span>
            <span className="green-dot" /> Your space. Your pace.
          </span>
          <span>Always free. Always ad-free.</span>
        </footer>
      </main>
      {panel === "search" && peer && (
        <SearchPanel search={useChatStore.getState().searchAll} senderName={(message) => (message.senderId === authUser._id ? "You" : peer.fullName)} onClose={() => setPanel(null)} />
      )}
      {panel && panel !== "search" && (
        <div className="modal-backdrop" onClick={() => setPanel(null)}>
          <section
            className="detail-modal"
            role="dialog"
            aria-modal="true"
            aria-label={panel === "people" ? "People" : "Privacy details"}
            onClick={(e) => e.stopPropagation()}
          >
            <button
              className="modal-close icon-button"
              aria-label="Close"
              onClick={() => setPanel(null)}
            >
              <X size={20} />
            </button>
            {panel === "group" ? (
              <>
                <Users className="accent" size={28} />
                <h2>Bring a few friends together.</h2>
                <p>Pick 2 to 9 friends. Group chats are encrypted for each person and have a voice room.</p>
                <form onSubmit={async (e) => {
                  e.preventDefault();
                  if (await useCommunityStore.getState().createGroup(groupPicks, groupName)) { setPanel(null); setCommunityMode(true); }
                }}>
                  <label className="dialog-field">Group name (optional)
                    <input maxLength={60} value={groupName} onChange={(e) => setGroupName(e.target.value)} />
                  </label>
                  <div className="modal-people">
                    {users.map((user) => (
                      <label className="settings-checkbox" key={user._id}>
                        <input type="checkbox" checked={groupPicks.includes(user._id)} disabled={!groupPicks.includes(user._id) && groupPicks.length >= 9} onChange={(e) => setGroupPicks(e.target.checked ? [...groupPicks, user._id] : groupPicks.filter((id) => id !== user._id))} />
                        <Avatar user={user} /> {user.fullName}
                      </label>
                    ))}
                    {!users.length && <p>Add some friends first.</p>}
                  </div>
                  <button className="primary" disabled={groupPicks.length < 2}>Create group <Check size={16} /></button>
                </form>
              </>
            ) : panel === "people" ? (
              <>
                <Users className="accent" size={28} />
                <h2>Your people, a little closer.</h2>
                <p>
                  Add someone by their username. Once they accept, you can
                  message and call each other.
                </p>
                <form
                  className="friend-add"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (peopleQuery.trim() && await useChatStore.getState().sendFriendRequest(peopleQuery, friendNote)) { setPeopleQuery(""); setFriendNote(""); }
                  }}
                >
                  <label className="search-box modal-search">
                    <Search size={15} />
                    <input
                      aria-label="Friend's username"
                      placeholder="Their username"
                      value={peopleQuery}
                      onChange={(e) => setPeopleQuery(e.target.value)}
                    />
                  </label>
                  <button className="primary compact" disabled={!peopleQuery.trim()}>Send request</button>
                </form>
                <input className="friend-note-input" aria-label="Note to include" placeholder="Add a short note, like where you met (optional)" maxLength={200} value={friendNote} onChange={(e) => setFriendNote(e.target.value)} />
                <div className="modal-people">
                  {friendRequests.incoming.length > 0 && <span className="eyebrow">FRIEND REQUESTS</span>}
                  {friendRequests.incoming.map((user) => (
                    <div className="person friend-request" key={user._id}>
                      <Avatar user={user} />
                      <span>
                        <strong>{user.fullName}</strong>
                        <small>@{user.username}</small>
                        {user.note && <FriendNote me={authUser._id} person={user} />}
                      </span>
                      <button className="primary compact" onClick={() => useChatStore.getState().answerFriendRequest(user._id, true)}>Accept</button>
                      <button className="secondary compact" onClick={() => useChatStore.getState().answerFriendRequest(user._id, false)}>Decline</button>
                    </div>
                  ))}
                  {friendRequests.outgoing.length > 0 && <span className="eyebrow">WAITING FOR A REPLY</span>}
                  {friendRequests.outgoing.map((user) => (
                    <div className="person friend-request" key={user._id}>
                      <Avatar user={user} />
                      <span>
                        <strong>{user.fullName}</strong>
                        <small>@{user.username}</small>
                      </span>
                      <button className="secondary compact" onClick={() => useChatStore.getState().answerFriendRequest(user._id, false)}>Cancel</button>
                    </div>
                  ))}
                  {users.length > 0 && <span className="eyebrow">YOUR FRIENDS</span>}
                  {users.map((user) => (
                    <div className="friend-row" key={user._id}>
                      <button
                        className="person"
                        onClick={() => {
                          setSelectedUser(user);
                          setPanel(null);
                        }}
                      >
                        <Avatar user={user} dot={onlineUsers.includes(user._id)} />
                        <span>
                          <strong>{user.fullName}</strong>
                          {user.username && <small>@{user.username}</small>}
                        </span>
                        <ArrowUpRight size={17} />
                      </button>
                      <button className={`icon-button ${preferences.pinnedPeople.includes(user._id) ? "accent" : ""}`} title="Pin to the top" aria-label={`Pin ${user.fullName}`} onClick={() => preferences.togglePinned(user._id)}><Pin size={15} /></button>
                      <select className="member-actions" aria-label={`Folder for ${user.fullName}`} value={preferences.folders.find((folder) => folder.people.includes(user._id))?.name || ""} onChange={(e) => {
                        const name = e.target.value === "__new" ? window.prompt("Folder name")?.trim().slice(0, 30) : e.target.value;
                        if (name !== undefined) preferences.moveToFolder(user._id, name);
                      }}>
                        <option value="">No folder</option>
                        {preferences.folders.map((folder) => <option key={folder.name} value={folder.name}>{folder.name}</option>)}
                        <option value="__new">New folder...</option>
                      </select>
                      <ActionMenu label={`More for ${user.fullName}`} items={[
                        { label: "Remove friend", icon: <UserMinus size={15} />, onClick: () => { if (window.confirm(`Remove ${user.fullName} from your friends? You will stop seeing each other here. Either of you can send a new request later.`)) useChatStore.getState().removeFriend(user._id); } },
                        { label: "Block", icon: <Ban size={15} />, danger: true, onClick: () => { if (window.confirm(`Block ${user.fullName}? They cannot message, call or send you requests. They will not be told.`)) preferences.block(user._id); } },
                      ]} />
                    </div>
                  ))}
                  {!users.length && !friendRequests.incoming.length && !friendRequests.outgoing.length && (
                    <p>No friends yet. Send a request to get started.</p>
                  )}
                </div>
              </>
            ) : (
              <>
                <ShieldCheck className="accent" size={30} />
                <span className="eyebrow">PRIVACY, IN PLAIN SIGHT</span>
                <h2>
                  {panel === "account"
                    ? authUser.fullName
                    : "Keep the connection. Lose the noise."}
                </h2>
                <div className="privacy-item">
                  <LockKeyhole />
                  <div>
                    <strong>Encrypted before it leaves.</strong>
                    <p>
                      Message content is encrypted on your device. Your key is
                      locked with your recovery key before it is stored, so only
                      your devices can open it. Contact keys are remembered and
                      changes block sending. Compare safety codes with a contact
                      in person or over a call to verify them.
                    </p>
                  </div>
                </div>
                <div className="privacy-item">
                  <Timer />
                  <div>
                    <strong>Seven days. Then gone.</strong>
                    <p>
                      Unpinned messages leave history at expiry and are deleted
                      by the database cleanup process. Pin something to keep it.
                      Unpinning starts a fresh seven days.
                    </p>
                  </div>
                </div>
                <div className="privacy-item">
                  <Headphones />
                  <div>
                    <strong>Voice is for the moment.</strong>
                    <p>
                      Direct WebRTC audio is encrypted in transit. Sonar has no
                      recording or media storage. Calls need a compatible
                      network or a configured STUN/TURN service.
                    </p>
                  </div>
                </div>
                {peer && print && (
                  <div className="fingerprint">
                    <strong>{peer.fullName}’s safety code</strong>
                    <code>{print}</code>
                    {contactTrust?.changed && <>
                      <p>{peer.fullName.split(" ")[0]}’s key changed, for example because they reset it or set up a new device. Ask them to open Account settings → Privacy &amp; device and read you their safety code in person or on a call. If it matches the code above, trust the new key. Older messages may become unreadable.</p>
                      <button type="button" disabled={verifyingContact} onClick={verifyContactChange}>The codes match, trust the new key</button>
                    </>}
                    {trustNotice && <p role="status">{trustNotice}</p>}
                  </div>
                )}
                {peer && import.meta.env.VITE_SONAR_MATRIX_EXPERIMENTAL === "true" && <div>
                  <button type="button" className="secondary" onClick={() => setExperimentalPane("verify")}>Verify contact device</button>
                  <button type="button" className="secondary" onClick={() => setExperimentalPane("messages")}>Open Matrix test messages</button>
                  {experimentalPane === "verify" ? <ExperimentalContactVerification key={peer._id} accountId={authUser._id} peer={peer} /> : <ExperimentalMatrixChat key={peer._id} accountId={authUser._id} peer={peer} socket={socket} />}
                </div>}
                <div className="honest-note">
                  <Check size={16} />
                  <p>
                    This is an early implementation, not an audited messenger.
                    Account and connection metadata remain visible to the
                    service. Forward secrecy is not implemented. Recipients can
                    copy or record content.
                  </p>
                </div>
                <button className="primary" onClick={() => setPanel(null)}>
                  Got it <Check size={16} />
                </button>
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
