import { create } from "zustand";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";
import { useAuthStore } from "./useAuthStore";
import { encrypt, decrypt } from "../lib/crypto";
import { mergeHistory, withoutExpired } from "../lib/history";
import { deleteFile } from "../lib/attachments";
import { encodeFile, encodeMessage, encodeMissedCall, encodeReaction, MISSED_CALL_TEXT } from "../lib/messageContent";
import { useUnreadStore } from "./useUnreadStore";
import { messageAlert } from "../lib/messageAlert";
import { refreshedContact } from "../lib/contacts";
import { candidatePosition, newer, readPosition, rememberPosition } from "../lib/offlineUnread";
let generation = 0;
let usersGeneration = 0;
const deleted = new Set();
const visibleMessages = (messages) => withoutExpired(messages).filter((message) => !deleted.has(message._id));
const fail = (e) => toast.error(e.response?.data?.message || e.message);
export const useChatStore = create((set, get) => ({
  nextCursor: null,
  hasMore: false,
  loadingOlder: false,
  pinnedOnly: false,
  pruneExpired: (now = Date.now()) => {
    const current = get().messages;
    const retained = withoutExpired(current, now);
    if (retained.length !== current.length) set({ messages: retained });
  },
  reset: () => {
    deleted.clear();
    generation++;
    usersGeneration++;
    set({
      users: [],
      isUsersLoading: false,
      messages: [],
      selectedUser: null,
      nextCursor: null,
      hasMore: false,
      loadingOlder: false,
      isMessagesLoading: false,
      pinnedOnly: false,
    });
  },
  users: [],
  messages: [],
  selectedUser: null,
  isUsersLoading: false,
  isMessagesLoading: false,
  recordVisibleDirect: () => {
    const peer = get().selectedUser?._id, me = useAuthStore.getState().authUser?._id;
    if (!peer || !me || get().pinnedOnly || useUnreadStore.getState().activePeer !== peer) return;
    let latest = null;
    for (const message of get().messages) {
      if (message.senderId !== peer || message.unreadable) continue;
      try {
        const value = candidatePosition({ messageId: message._id, createdAt: message.createdAt });
        if (newer(value, latest)) latest = value;
      } catch { /* Ignore invalid metadata. */ }
    }
    if (latest) rememberPosition(me, peer, latest).catch(() => {});
  },
  refreshOfflineUnread: async (accountId) => {
    try {
      const revision = useUnreadStore.getState().directRevision;
      const unread = new Set();
      let complete = true, after = null;
      do {
        const { data } = await api.get("/messages/unread-candidates", { params: after ? { after } : {} });
        if (!Array.isArray(data?.items) || (data.next !== null && (typeof data.next !== "string" || (after && data.next <= after))) || useAuthStore.getState().authUser?._id !== accountId) return;
        for (const candidate of data.items) {
          try {
            const latest = candidatePosition(candidate);
            const seen = await readPosition(accountId, candidate.senderId);
            if (useAuthStore.getState().authUser?._id !== accountId) return;
            if (newer(latest, seen)) unread.add(candidate.senderId);
          } catch { complete = false; }
        }
        after = data.next;
      } while (after);
      // Do not clear a notification that arrived while this snapshot was loading.
      if (complete && useUnreadStore.getState().directRevision === revision)
        for (const id of Object.keys(useUnreadStore.getState().direct)) if (!unread.has(id)) useUnreadStore.getState().clearDirect(id);
      for (const id of unread) useUnreadStore.getState().markDirect(id);
    } catch { /* Live indicators remain available while offline. */ }
  },
  getUsers: async () => {
    const token = ++usersGeneration;
    const accountId = useAuthStore.getState().authUser?._id;
    set({ isUsersLoading: true });
    try {
      const users = (await api.get("/messages/users")).data;
      if (token !== usersGeneration || !accountId || useAuthStore.getState().authUser?._id !== accountId) return;
      const selected = get().selectedUser;
      const refreshed = refreshedContact(selected, users);
      if (selected && !refreshed) get().setSelectedUser(null);
      set({ users, selectedUser: refreshed });
      void get().refreshOfflineUnread(accountId);
    } catch (e) {
      if (token === usersGeneration) fail(e);
    } finally {
      if (token === usersGeneration) set({ isUsersLoading: false });
    }
  },
  friendRequests: { incoming: [], outgoing: [] },
  getFriendRequests: async () => {
    try {
      set({ friendRequests: (await api.get("/friends")).data });
    } catch { /* The next refresh tries again. */ }
  },
  sendFriendRequest: async (username) => {
    try {
      const { data } = await api.post("/friends/request", { username });
      toast.success(data.status === "accepted" ? "You're friends now" : "Friend request sent");
      return true;
    } catch (e) {
      toast.error(e.response?.data?.message || "Could not send the friend request");
      return false;
    }
  },
  answerFriendRequest: async (id, accept) => {
    try {
      await (accept ? api.post(`/friends/${id}/accept`) : api.delete(`/friends/${id}`));
    } catch (e) {
      toast.error(e.response?.data?.message || "That request is no longer available");
    }
    await Promise.all([get().getUsers(), get().getFriendRequests()]);
  },
  setSelectedUser: (user) => {
    deleted.clear();
    generation++;
    set({
      selectedUser: user,
      messages: [],
      nextCursor: null,
      hasMore: false,
      loadingOlder: false,
      pinnedOnly: false,
    });
  },
  getMessages: async (id, pinnedOnly = false) => {
    const token = ++generation;
    set({
      isMessagesLoading: true,
      messages: [],
      pinnedOnly,
      hasMore: false,
      nextCursor: null,
      loadingOlder: false,
    });
    try {
      const peer = get().selectedUser;
      const me = useAuthStore.getState().authUser._id;
      const { data } = await api.get(`/messages/${id}`, {
        params: { page: true, pinned: pinnedOnly },
      });
      const messages = await Promise.all(
        data.messages.map((message) => decrypt(me, peer, message)),
      );
      if (token === generation && get().selectedUser?._id === id) {
        set({
          messages: visibleMessages(mergeHistory(messages, get().messages)).filter(
            (m) => !pinnedOnly || m.pinned,
          ),
          nextCursor: data.nextCursor,
          hasMore: data.hasMore,
        });
        get().recordVisibleDirect();
      }
    } catch (error) {
      fail(error);
    } finally {
      if (token === generation) set({ isMessagesLoading: false });
    }
  },
  loadOlder: async () => {
    const {
      selectedUser: peer,
      nextCursor,
      pinnedOnly,
      hasMore,
      loadingOlder,
    } = get();
    if (!peer || !hasMore || loadingOlder) return;
    const token = generation;
    set({ loadingOlder: true });
    try {
      const me = useAuthStore.getState().authUser._id;
      const { data } = await api.get(`/messages/${peer._id}`, {
        params: { page: true, before: nextCursor, pinned: pinnedOnly },
      });
      const messages = await Promise.all(
        data.messages.map((message) => decrypt(me, peer, message)),
      );
      if (token === generation)
        set({
          messages: visibleMessages(mergeHistory(messages, get().messages)),
          nextCursor: data.nextCursor,
          hasMore: data.hasMore,
        });
    } catch (error) {
      fail(error);
    } finally {
      if (token === generation) set({ loadingOlder: false });
    }
  },
  sendMessage: async ({ text, replyTo, reaction, file }) => {
    try {
      const peer = get().selectedUser;
      const me = useAuthStore.getState().authUser._id;
      const envelope = await encrypt(me, peer, file ? encodeFile(file) : reaction ? encodeReaction(reaction) : encodeMessage(text, replyTo));
      if (get().selectedUser?._id !== peer._id || get().selectedUser?.encryptionKey !== peer.encryptionKey || useAuthStore.getState().authUser?._id !== me)
        throw new Error("The conversation or contact key changed. Review the contact before sending.");
      const { data } = await api.post(`/messages/send/${peer._id}`, envelope);
      if (
        !get().pinnedOnly &&
        get().selectedUser?._id === peer._id &&
        !get().messages.some((m) => m._id === data._id)
      )
        set({ messages: visibleMessages(mergeHistory(get().messages, [{ ...data, text: reaction ? "" : text, ...(replyTo ? { replyTo } : {}), ...(reaction ? { reaction } : {}), ...(file ? { file } : {}) }])) });
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  },
  // Best effort and silent: a failure here must not disturb the end of a call.
  sendMissedCall: async (peerId) => {
    try {
      const peer = get().users.find((user) => user._id === peerId);
      const me = useAuthStore.getState().authUser?._id;
      if (!peer?.encryptionKey || !me) return;
      const envelope = await encrypt(me, peer, encodeMissedCall());
      const { data } = await api.post(`/messages/send/${peer._id}`, envelope);
      if (!get().pinnedOnly && get().selectedUser?._id === peer._id && !get().messages.some((m) => m._id === data._id))
        set({ messages: visibleMessages(mergeHistory(get().messages, [{ ...data, text: MISSED_CALL_TEXT, call: "missed" }])) });
    } catch { /* The call has already ended. */ }
  },
  deleteMessage: async (message) => {
    try {
      const { data } = await api.delete(`/messages/${message._id}`);
      if (get().selectedUser?._id === data.receiverId) {
        deleted.add(data.id);
        set({ messages: get().messages.filter(item => item._id !== data.id) });
      }
      if (message.file) deleteFile(message.file.id);
    } catch (e) { fail(e); }
  },
  pinMessage: async (message) => {
    try {
      const { data } = await api.patch(`/messages/${message._id}/pin`, {
        pinned: !message.pinned,
      });
      const peer = get().selectedUser;
      if (peer && !deleted.has(data._id) && [data.senderId, data.receiverId].includes(peer._id))
        set({
          messages:
            get().pinnedOnly && !data.pinned
              ? get().messages.filter((item) => item._id !== data._id)
              : visibleMessages(mergeHistory(get().messages, [{ ...data, text: message.text, ...(message.replyTo ? { replyTo: message.replyTo } : {}) }])),
        });
    } catch (e) {
      fail(e);
    }
  },
  subscribeToMessages: () => {
    const socket = useAuthStore.getState().socket;
    if (!socket) return;
    const receive = async (message) => {
      const peer = get().selectedUser;
      const me = useAuthStore.getState().authUser?._id;
      if (!peer || ![message.senderId, message.receiverId].includes(peer._id))
        return;
      const decoded = await decrypt(me, peer, message);
      if (get().selectedUser?._id !== peer._id || deleted.has(message._id)) return;
      set({
        messages:
          get().pinnedOnly && !decoded.pinned
            ? get().messages.filter((m) => m._id !== decoded._id)
            : visibleMessages(mergeHistory(get().messages, [decoded])),
      });
      get().recordVisibleDirect();
    };
    const remove = ({ id, senderId, receiverId }) => {
      const peer = get().selectedUser;
      if (!peer || ![senderId, receiverId].includes(peer._id)) return;
      deleted.add(id);
      set({ messages: get().messages.filter(item => item._id !== id) });
    };
    const onNewMessage = async (message) => {
      const me = useAuthStore.getState().authUser?._id;
      if (message.receiverId === me && message.senderId !== me) {
        const unread = useUnreadStore.getState();
        const first = !unread.direct[message.senderId];
        const sender = get().users.find((user) => user._id === message.senderId);
        // Reactions are messages too, but nobody needs an alert for one.
        const decoded = sender ? await decrypt(me, sender, message).catch(() => null) : null;
        if (decoded?.reaction) return receive(message);
        unread.markDirect(message.senderId);
        if (useUnreadStore.getState().activePeer !== message.senderId)
          messageAlert(`New message from ${sender?.fullName || "someone"}`);
        if (first && unread.notificationsEnabled && document.hidden &&
            typeof Notification !== "undefined" && Notification.permission === "granted") {
          try { new Notification("Sonar", { body: "You have a new message.", tag: "sonar-new-message" }); } catch { /* Unsupported browser */ }
        }
      }
      receive(message);
    };
    socket.on("messageDeleted", remove);
    socket.on("newMessage", onNewMessage);
    socket.on("messageUpdated", receive);
  },
  unsubscribeFromMessages: () => {
    const socket = useAuthStore.getState().socket;
    socket?.off("messageDeleted");
    socket?.off("newMessage");
    socket?.off("messageUpdated");
  },
}));
