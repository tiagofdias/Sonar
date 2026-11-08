import { create } from "zustand";
// Deliberately memory-only: drafts never go to localStorage or the server.
export const useDraftStore = create((set, get) => ({
  drafts: {},
  update: (key, value) => {
    if (!key) return;
    const drafts = { ...get().drafts };
    if (value) drafts[key] = value;
    else delete drafts[key];
    const keys = Object.keys(drafts);
    if (keys.length > 100) delete drafts[keys[0]];
    set({ drafts });
  },
  clearPrefix: (prefix) =>
    set({
      drafts: Object.fromEntries(
        Object.entries(get().drafts).filter(([key]) => !key.startsWith(prefix)),
      ),
    }),
  reset: () => set({ drafts: {} }),
}));
export function useConversationDraft(key) {
  const text = useDraftStore((state) => (key ? state.drafts[key] || "" : ""));
  return {
    text,
    setText: (value) => useDraftStore.getState().update(key, value),
    clearSent: (sent) => {
      if (useDraftStore.getState().drafts[key] === sent)
        useDraftStore.getState().update(key, "");
    },
  };
}
