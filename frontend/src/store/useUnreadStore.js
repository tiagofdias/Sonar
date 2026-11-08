import { create } from "zustand";
// Indicators and local read positions do not create read receipts.
export const useUnreadStore = create((set, get) => ({
  direct: {},
  channels: {},
  activeChannel: null,
  activePeer: null,
  directRevision: 0,
  channelRevision: 0,
  notificationsEnabled: false,
  setNotificationsEnabled: (enabled) => set({ notificationsEnabled: enabled }),
  setActivePeer: (id) => {
    set({ activePeer: id });
    if (id) get().clearDirect(id);
  },
  markDirect: (id) => {
    if (!id || get().activePeer === id) return;
    set(state => ({ direct: { ...state.direct, [id]: true }, directRevision: state.directRevision + 1 }));
  },
  clearDirect: (id) => set(state => {
    if (!state.direct[id]) return state;
    const direct = { ...state.direct };
    delete direct[id];
    return { direct };
  }),
  setActiveChannel: (key) => {
    set({ activeChannel: key });
    if (key) get().clearChannel(key);
  },
  markChannel: (key) => {
    if (!key || get().activeChannel === key) return;
    set(state => ({ channels: { ...state.channels, [key]: true }, channelRevision: state.channelRevision + 1 }));
  },
  clearChannel: (key) => set(state => {
    if (!state.channels[key]) return state;
    const channels = { ...state.channels };
    delete channels[key];
    return { channels };
  }),
  clearCommunity: (id) => set(state => ({
    channels: Object.fromEntries(Object.entries(state.channels).filter(([key]) => !key.startsWith(`${id}:`))),
    channelRevision: state.channelRevision + 1,
  })),
  reset: () => set(state => ({ direct: {}, channels: {}, activePeer: null, activeChannel: null, notificationsEnabled: false, directRevision: state.directRevision + 1, channelRevision: state.channelRevision + 1 })),
}));
