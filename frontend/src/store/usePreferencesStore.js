import { create } from "zustand";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";

const KEY = "sonar-device-settings";
const device = () => {
  try { return JSON.parse(localStorage.getItem(KEY)) || {}; } catch { return {}; }
};
const fail = (error) => toast.error(error.response?.data?.message || "That could not be saved");

export const usePreferencesStore = create((set, get) => ({
  mutedCommunities: [],
  pinnedPeople: [],
  folders: [],
  typingHidden: false,
  blocked: [],
  // These stay on this device. Keywords in particular never reach the server.
  alertVolume: device().alertVolume ?? 0.8,
  confirmCalls: device().confirmCalls ?? false,
  keywords: device().keywords ?? [],
  setDevice: (patch) => {
    set(patch);
    const { alertVolume, confirmCalls, keywords } = get();
    try { localStorage.setItem(KEY, JSON.stringify({ alertVolume, confirmCalls, keywords })); } catch { /* Private browsing. */ }
  },
  load: async () => {
    try { set((await api.get("/preferences")).data); } catch { /* The next refresh tries again. */ }
  },
  save: async (patch) => {
    const before = get();
    set(patch);
    try { await api.put("/preferences", patch); return true; }
    catch (error) {
      set(Object.fromEntries(Object.keys(patch).map((name) => [name, before[name]])));
      fail(error);
      return false;
    }
  },
  block: async (id) => {
    try { set((await api.post(`/preferences/block/${id}`)).data); return true; } catch (error) { fail(error); return false; }
  },
  unblock: async (id) => {
    try { set((await api.delete(`/preferences/block/${id}`)).data); return true; } catch (error) { fail(error); return false; }
  },
  isBlocked: (id) => get().blocked.some((person) => person._id === id),
  isMuted: (id) => get().mutedCommunities.includes(id),
  toggleMute: (id) => get().save({ mutedCommunities: get().isMuted(id) ? get().mutedCommunities.filter((item) => item !== id) : [...get().mutedCommunities, id] }),
  togglePinned: (id) => get().save({ pinnedPeople: get().pinnedPeople.includes(id) ? get().pinnedPeople.filter((item) => item !== id) : [...get().pinnedPeople, id] }),
  moveToFolder: (person, name) => {
    const without = get().folders.map((folder) => ({ ...folder, people: folder.people.filter((id) => id !== person) }));
    if (name && !without.some((folder) => folder.name === name)) without.push({ name, people: [] });
    return get().save({ folders: without.map((folder) => (folder.name === name ? { ...folder, people: [...folder.people, person] } : folder)).filter((folder) => folder.people.length) });
  },
}));

export function matchesKeyword(text, keywords) {
  const value = (text || "").toLowerCase();
  return keywords.some((word) => word && value.includes(word.toLowerCase()));
}
