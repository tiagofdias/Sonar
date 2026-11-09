import { deleteFile } from "../lib/attachments";
import { create } from "zustand";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";
import { useAuthStore } from "./useAuthStore";
import { encrypt, decrypt } from "../lib/crypto";
import { mergeHistory, withoutExpired } from "../lib/history";
import { encodeFile, encodeMessage, encodeReaction } from "../lib/messageContent";
import { useDraftStore } from "./useDraftStore";
import { searchHistory } from "../lib/search";
import { useUnreadStore } from "./useUnreadStore";
import { candidatePosition, newer, readChannelPosition, rememberChannelPosition } from "../lib/offlineUnread";
const fail = (error) =>
  toast.error(error.response?.data?.message || error.message);
const base = (community) => `/communities/${community._id}`;
const upsert = (items, item) => visibleMessages(mergeHistory(items, [item]));
let generation = 0;
const detailsCache = new Map();
const deleted = new Set();
const visibleMessages = (messages) => withoutExpired(messages).filter(message => !deleted.has(message._id));
export const useCommunityStore = create((set, get) => ({
  communities: [],
  community: null,
  channel: null,
  messages: [],
  loading: false,
  nextCursor: null,
  hasMore: false,
  loadingOlder: false,
  pinnedOnly: false,
  recordVisibleChannel: () => {
    const community = get().community?._id, channel = get().channel?._id, me = useAuthStore.getState().authUser?._id;
    if (!community || !channel || !me || get().pinnedOnly || useUnreadStore.getState().activeChannel !== `${community}:${channel}`) return;
    let latest = null;
    for (const message of get().messages) {
      if (message.senderId === me || message.unreadable) continue;
      try {
        const value = candidatePosition({ messageId: message._id, createdAt: message.createdAt });
        if (newer(value, latest)) latest = value;
      } catch { /* Ignore invalid metadata. */ }
    }
    if (latest) rememberChannelPosition(me, community, channel, latest).catch(() => {});
  },
  refreshOfflineUnread: async (accountId, communities) => {
    const revision = useUnreadStore.getState().channelRevision;
    const unread = new Set();
    let complete = true;
    for (const community of communities) {
      try {
        const { data } = await api.get(`/communities/${community._id}/unread-candidates`);
        if (!Array.isArray(data)) { complete = false; continue; }
        for (const candidate of data) {
          const latest = candidatePosition(candidate);
          const seen = await readChannelPosition(accountId, community._id, candidate.channelId);
          if (useAuthStore.getState().authUser?._id !== accountId) return;
          if (newer(latest, seen)) unread.add(`${community._id}:${candidate.channelId}`);
        }
      } catch { complete = false; }
    }
    if (useAuthStore.getState().authUser?._id !== accountId) return;
    const allowed = new Set(get().communities.flatMap(item => item.channels.map(channel => `${item._id}:${channel._id}`)));
    if (complete && useUnreadStore.getState().channelRevision === revision)
      for (const key of Object.keys(useUnreadStore.getState().channels)) if (!unread.has(key)) useUnreadStore.getState().clearChannel(key);
    for (const key of unread) if (allowed.has(key)) useUnreadStore.getState().markChannel(key);
  },
  pruneExpired: (now = Date.now()) => {
    const current = get().messages;
    const retained = withoutExpired(current, now);
    if (retained.length !== current.length) set({ messages: retained });
  },
  removeAccess: (communityId) => {
    useDraftStore.getState().clearPrefix(`channel:${communityId}:`);
    const patch = {
      communities: get().communities.filter((item) => item._id !== communityId),
    };
    if (get().community?._id === communityId) {
      generation++;
      Object.assign(patch, {
        community: null,
        channel: null,
        messages: [],
        loading: false,
        nextCursor: null,
        hasMore: false,
        loadingOlder: false,
      });
    }
    set(patch);
  },
  manageMember: async (action, memberId) => {
    const community = get().community;
    if (!community) return false;
    try {
      if (action === "leave") await api.post(`${base(community)}/leave`);
      else if (action === "remove")
        await api.delete(`${base(community)}/members/${memberId}`);
      else if (action === "ban")
        await api.post(`${base(community)}/members/${memberId}/ban`);
      else if (action === "unban")
        await api.delete(`${base(community)}/bans/${memberId}`);
      else if (action === "transfer")
        await api.post(`${base(community)}/transfer`, { memberId });
      else if (action === "promote" || action === "demote")
        await api.put(`${base(community)}/members/${memberId}/moderator`, { moderator: action === "promote" });
      else return false;
      if (action === "leave") get().removeAccess(community._id);
      else if (get().community?._id === community._id) await get().refresh();
      await get().load();
      return true;
    } catch (error) {
      fail(error);
      return false;
    }
  },
  reset: () => {
    deleted.clear();
    generation++;
    set({
      communities: [],
      community: null,
      channel: null,
      messages: [],
      loading: false,
      nextCursor: null,
      hasMore: false,
      loadingOlder: false,
      pinnedOnly: false,
    });
  },
  load: async () => {
    const me = useAuthStore.getState().authUser?._id;
    try {
      const { data } = await api.get("/communities");
      if (me && useAuthStore.getState().authUser?._id === me) {
        set({ communities: data });
        void get().refreshOfflineUnread(me, data);
        const selected = get().community;
        if (selected && !data.some((item) => item._id === selected._id))
          get().removeAccess(selected._id);
      }
    } catch (error) {
      fail(error);
    }
  },
  select: async (communityId) => {
    const token = ++generation;
    set({ loading: true, messages: [], channel: null, community: null });
    try {
      const { data } = await api.get(`/communities/${communityId}`);
      if (token !== generation) return;
      set({ community: data });
      await get().selectChannel(
        data.channels.find((channel) => channel.type !== "voice"),
      );
    } catch (error) {
      fail(error);
    } finally {
      if (token === generation) set({ loading: false });
    }
  },
  refresh: async () => {
    const selected = get().community;
    if (!selected) return;
    try {
      const { data } = await api.get(base(selected));
      if (get().community?._id === data._id) {
        const channel = get().channel;
        for (const oldChannel of get().community.channels) {
          if (!data.channels.some(item => item._id === oldChannel._id))
            useDraftStore.getState().update(`channel:${data._id}:${oldChannel._id}`, "");
        }
        const selectedChannel = channel && data.channels.find(item => item._id === channel._id);
        set({ community: data, channel: selectedChannel || null });
        if (channel && !selectedChannel) {
          useDraftStore.getState().update(`channel:${data._id}:${channel._id}`, "");
          generation++;
          set({ messages: [], nextCursor: null, hasMore: false, loadingOlder: false, loading: false });
          await get().selectChannel(data.channels.find(item => item.type !== "voice"));
        }
      }
      return true;
    } catch (error) {
      if (error.response?.status === 404) get().removeAccess(selected._id);
      fail(error);
      return false;
    }
  },
  create: async (name) => {
    try {
      const { data } = await api.post("/communities", { name });
      await get().load();
      await get().select(data._id);
      return true;
    } catch (error) {
      fail(error);
      return false;
    }
  },
  join: async (token) => {
    try {
      const { data } = await api.post("/communities/join", { token });
      await get().load();
      await get().select(data._id);
      return true;
    } catch (error) {
      fail(error);
      return false;
    }
  },
  createChannel: async (name, type = "text") => {
    try {
      const community = get().community;
      const { data } = await api.post(`${base(community)}/channels`, {
        name,
        type,
      });
      if (get().community?._id === community._id) {
        await get().refresh();
        if (type === "text") await get().selectChannel(data.channels.at(-1));
      }
      return true;
    } catch (error) {
      fail(error);
      return false;
    }
  },
  setVoiceLock: async (channelId, locked) => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.patch(`${base(community)}/channels/${channelId}/voice-lock`, { locked });
      if (get().community?._id === community._id) await get().refresh();
      return true;
    } catch (error) { fail(error); return false; }
  },
  getBans: async () => {
    const community = get().community;
    if (!community) return null;
    try { return (await api.get(`${base(community)}/bans`)).data; }
    catch (error) { fail(error); return null; }
  },
  deleteCommunity: async () => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.delete(base(community));
      get().removeAccess(community._id);
      await get().load();
      return true;
    } catch (error) { fail(error); return false; }
  },
  deleteChannel: async (channelId) => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.delete(`${base(community)}/channels/${channelId}`);
      useDraftStore.getState().update(`channel:${community._id}:${channelId}`, "");
      if (get().community?._id === community._id) await get().refresh();
      await get().load();
      return true;
    } catch (error) { fail(error); return false; }
  },
  rename: async (name, channelId) => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.patch(`${base(community)}${channelId ? `/channels/${channelId}` : ""}`, { name });
      if (get().community?._id === community._id) await get().refresh();
      await get().load();
      return true;
    } catch (error) { fail(error); return false; }
  },
  invite: async () => {
    try {
      return (await api.post(`${base(get().community)}/invite`)).data;
    } catch (error) {
      fail(error);
      return null;
    }
  },
  selectChannel: async (channel, pinnedOnly = false) => {
    const community = get().community;
    if (!community || !channel || channel.type === "voice") return;
    if (get().channel?._id !== channel._id) deleted.clear();
    const token = ++generation;
    set({
      channel,
      messages: [],
      loading: true,
      nextCursor: null,
      hasMore: false,
      loadingOlder: false,
      pinnedOnly,
    });
    try {
      const me = useAuthStore.getState().authUser._id;
      const { data } = await api.get(
        `${base(community)}/channels/${channel._id}/messages`,
        { params: { page: true, pinned: pinnedOnly } },
      );
      const messages = await Promise.all(
        data.messages.map((message) =>
          decrypt(
            me,
            [
              ...community.memberDetails,
              ...(community.historyAuthors || []),
            ].find((m) => m._id === message.senderId),
            message,
          ),
        ),
      );
      if (token === generation) {
        set({
          messages: visibleMessages(mergeHistory(messages, get().messages)).filter(
            (m) => !pinnedOnly || m.pinned,
          ),
          nextCursor: data.nextCursor,
          hasMore: data.hasMore,
        });
        get().recordVisibleChannel();
      }
    } catch (error) {
      fail(error);
    } finally {
      if (token === generation) set({ loading: false });
    }
  },
  loadOlder: async () => {
    const {
      community,
      channel,
      nextCursor,
      hasMore,
      loadingOlder,
      pinnedOnly,
    } = get();
    if (!community || !channel || !hasMore || loadingOlder) return;
    const token = generation;
    set({ loadingOlder: true });
    try {
      const me = useAuthStore.getState().authUser._id;
      const { data } = await api.get(
        `${base(community)}/channels/${channel._id}/messages`,
        { params: { page: true, before: nextCursor, pinned: pinnedOnly } },
      );
      const messages = await Promise.all(
        data.messages.map((message) =>
          decrypt(
            me,
            [
              ...community.memberDetails,
              ...(community.historyAuthors || []),
            ].find((m) => m._id === message.senderId),
            message,
          ),
        ),
      );
      if (token === generation)
        set({
          messages: visibleMessages(mergeHistory(messages, get().messages)),
          nextCursor: data.nextCursor,
          hasMore: data.hasMore,
        });
    } catch (error) {
      if (error.response?.status === 404) get().removeAccess(community._id);
      fail(error);
    } finally {
      if (token === generation) set({ loadingOlder: false });
    }
  },
  createGroup: async (memberIds, name) => {
    try {
      const { data } = await api.post("/communities/group", { memberIds, name });
      await get().load();
      await get().select(data._id);
      return true;
    } catch (error) { fail(error); return false; }
  },
  setDisappearAfter: async (channelId, seconds) => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.patch(`${base(community)}/channels/${channelId}/disappear`, { seconds });
      if (get().community?._id === community._id) await get().refresh();
      return true;
    } catch (error) { fail(error); return false; }
  },
  setModeratorLimits: async (memberId, denied) => {
    const community = get().community;
    if (!community) return false;
    try {
      await api.put(`${base(community)}/members/${memberId}/permissions`, { denied });
      if (get().community?._id === community._id) await get().refresh();
      return true;
    } catch (error) { fail(error); return false; }
  },
  getActivity: async () => {
    const community = get().community;
    if (!community) return null;
    try { return (await api.get(`${base(community)}/activity`)).data; }
    catch (error) { fail(error); return null; }
  },
  searchAll: async (query, stillWanted) => {
    const { community, channel } = get(), me = useAuthStore.getState().authUser._id;
    const people = [...community.memberDetails, ...(community.historyAuthors || [])];
    return searchHistory(
      async (before) => (await api.get(`${base(community)}/channels/${channel._id}/messages`, { params: { page: true, ...(before ? { before } : {}) } })).data,
      (message) => decrypt(me, people.find((person) => person._id === message.senderId), message), query, stillWanted,
    ).catch((error) => { fail(error); return []; });
  },
  edit: async (message, text, mentions = []) => {
    try {
      if (!(await get().refresh())) return false;
      const { community, channel } = get();
      if (!community || channel?._id !== message.channelId) return false;
      const me = useAuthStore.getState().authUser._id;
      const content = encodeMessage(text, message.replyTo, mentions);
      const envelopes = await Promise.all(community.memberDetails.map(async (peer) => ({ recipientId: peer._id, ...(await encrypt(me, peer, content, channel._id)) })));
      const { data } = await api.patch(`${base(community)}/channels/${channel._id}/messages/${message._id}`, { revision: community.revision, envelopes });
      if (get().channel?._id === channel._id)
        set({ messages: upsert(get().messages, { ...data, text, ...(message.replyTo ? { replyTo: message.replyTo } : {}), ...(mentions.length ? { mentions } : {}) }) });
      return true;
    } catch (error) { fail(error); return false; }
  },
  // Opens a message from any community, for deciding whether a muted server should still alert.
  openForAlert: async (message) => {
    try {
      let details = detailsCache.get(message.communityId);
      if (!details || !details.memberDetails.some((person) => person._id === message.senderId)) {
        details = (await api.get(`/communities/${message.communityId}`)).data;
        detailsCache.set(message.communityId, details);
      }
      const sender = details.memberDetails.find((person) => person._id === message.senderId);
      const decoded = await decrypt(useAuthStore.getState().authUser?._id, sender, message);
      const canEveryone = details.ownerId === message.senderId || details.moderators?.includes(message.senderId);
      return { decoded, canEveryone };
    } catch { return null; }
  },
  send: async (text, replyTo, reaction, file, mentions = []) => {
    try {
      const target = get().channel?._id;
      if (!(await get().refresh())) return false;
      const { community, channel } = get();
      if (!community || !channel || channel._id !== target) return false;
      const me = useAuthStore.getState().authUser._id;
      const envelopes = await Promise.all(
        community.memberDetails.map(async (peer) => ({
          recipientId: peer._id,
          ...(await encrypt(me, peer, file ? encodeFile(file) : reaction ? encodeReaction(reaction) : encodeMessage(text, replyTo, mentions), channel._id)),
        })),
      );
      const { data } = await api.post(
        `${base(community)}/channels/${channel._id}/messages`,
        { revision: community.revision, envelopes },
      );
      if (get().channel?._id === channel._id && !get().pinnedOnly)
        set({ messages: upsert(get().messages, { ...data, ...(file ? { file } : {}), text: reaction ? "" : text, ...(replyTo ? { replyTo } : {}), ...(reaction ? { reaction } : {}), ...(mentions.length ? { mentions } : {}) }) });
      return true;
    } catch (error) {
      fail(error);
      return false;
    }
  },
  removeMessage: ({ id, communityId, channelId }) => {
    if (get().community?._id !== communityId || get().channel?._id !== channelId) return;
    deleted.add(id);
    set({ messages: get().messages.filter(item => item._id !== id) });
  },
  deleteMessage: async (message) => {
    const community = get().community;
    if (!community) return;
    try {
      const { data } = await api.delete(`${base(community)}/channels/${message.channelId}/messages/${message._id}`);
      get().removeMessage(data);
      if (message.file) deleteFile(message.file.id);
    } catch (error) { fail(error); }
  },
  pin: async (message) => {
    try {
      const { data } = await api.patch(
        `${base(get().community)}/channels/${message.channelId}/messages/${message._id}/pin`,
        { pinned: !message.pinned },
      );
      if (get().channel?._id === data.channelId && !deleted.has(data._id))
        set({
          messages:
            get().pinnedOnly && !data.pinned
              ? get().messages.filter((m) => m._id !== data._id)
              : upsert(get().messages, { ...data, text: message.text, ...(message.replyTo ? { replyTo: message.replyTo } : {}) }),
        });
    } catch (error) {
      fail(error);
    }
  },
  receive: async (message) => {
    if (!message) return;
    const { community, channel } = get();
    if (
      message.channelId !== channel?._id ||
      message.communityId !== community?._id
    )
      return;
    let sender = [
      ...community.memberDetails,
      ...(community.historyAuthors || []),
    ].find((m) => m._id === message.senderId);
    if (!sender) {
      await get().refresh();
      const updated = get().community;
      sender =
        updated &&
        [...updated.memberDetails, ...(updated.historyAuthors || [])].find(
          (m) => m._id === message.senderId,
        );
    }
    const me = useAuthStore.getState().authUser?._id;
    const decoded = await decrypt(me, sender, message);
    if (get().channel?._id === message.channelId && !deleted.has(message._id)) {
      set({
        messages:
          get().pinnedOnly && !decoded.pinned
            ? get().messages.filter((m) => m._id !== decoded._id)
            : upsert(get().messages, decoded),
      });
      get().recordVisibleChannel();
    }
  },
}));
