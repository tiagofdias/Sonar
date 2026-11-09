import { create } from "zustand";
import { axiosInstance } from "../lib/axios.js";
import toast from "react-hot-toast";
import { LegacyIdentityMissing, IdentityLocked, pairLegacyIdentity, rotateIdentity, recoverAccount, replaceRecoveryKey } from "../lib/legacyIdentity.js";
import { io } from "socket.io-client";
import { useCommunityStore } from "./useCommunityStore";
import { useChatStore } from "./useChatStore";
import { useDraftStore } from "./useDraftStore";
import { useUnreadStore } from "./useUnreadStore";
import {
  openPasskeyWindow,
  runPasskey,
  passkeyRequest,
  passkeyError,
} from "../lib/passkeys";

const BASE_URL = window.location.origin;

export const useAuthStore = create((set, get) => ({
  authUser: null,
  unpairedUser: null,
  unpairedReason: null,
  // A recovery key just created on this device; shown once until the user saves it.
  newRecoveryKey: null,
  isSigningUp: false,
  isLoggingIn: false,
  isUpdatingProfile: false,
  isCheckingAuth: true,
  onlineUsers: [],
  presence: {},
  socket: null,
  settingsOpen: false,
  clearSession: () => {
    get().disconnectSocket();
    useCommunityStore.getState().reset();
    useChatStore.getState().reset();
    useDraftStore.getState().reset();
    useUnreadStore.getState().reset();
    set({ authUser: null, unpairedUser: null, unpairedReason: null, newRecoveryKey: null, settingsOpen: false });
  },

  // A signed-in account whose key is not on this device yet. Keeps the
  // session and asks for the password (locked) or explains how to continue.
  holdUnpaired: (error) => {
    if (error instanceof IdentityLocked) { set({ authUser: null, unpairedUser: error.user, unpairedReason: error.needs === "recovery" ? "recovery" : "locked" }); return true; }
    if (error instanceof LegacyIdentityMissing) { set({ authUser: null, unpairedUser: error.user, unpairedReason: "missing" }); return true; }
    return false;
  },
  showRecoveryKey: (key) => set({ newRecoveryKey: key }),
  dismissRecoveryKey: () => set({ newRecoveryKey: null }),
  // `secret` is the recovery key (current backups) or the password (legacy backups).
  unlockIdentity: async (secret) => {
    const user = get().unpairedUser;
    if (!user) return false;
    const recovery = get().unpairedReason === "recovery";
    const paired = await pairLegacyIdentity(user, axiosInstance, recovery ? undefined : secret, { recoveryKey: recovery ? secret : undefined, onRecoveryKey: get().showRecoveryKey });
    set({ authUser: paired, unpairedUser: null, unpairedReason: null });
    get().connectSocket();
    return true;
  },
  // Replaces the account key. Used from a device without the key (reset) or
  // from the original device to turn on multi-device access.
  rotateIdentity: async (password) => {
    const user = get().unpairedUser || get().authUser;
    if (!user) return false;
    const { user: rotated, recoveryKey } = await rotateIdentity(user, axiosInstance, password);
    useChatStore.getState().reset();
    useCommunityStore.getState().reset();
    set({ authUser: rotated, unpairedUser: null, unpairedReason: null, newRecoveryKey: recoveryKey });
    get().connectSocket();
    return true;
  },

  newRecoveryKeyFor: async (password) => {
    const user = get().authUser;
    if (!user) return false;
    set({ newRecoveryKey: await replaceRecoveryKey(user, axiosInstance, password) });
    return true;
  },
  // Forgot password: reset with the recovery key, then show a fresh one.
  recover: async (email, recoveryKey, newPassword) => {
    get().clearSession();
    const { user, recoveryKey: fresh } = await recoverAccount(axiosInstance, email, recoveryKey, newPassword);
    set({ authUser: user, unpairedUser: null, unpairedReason: null, newRecoveryKey: fresh });
    get().connectSocket();
    return true;
  },

  checkAuth: async () => {
    try {
      const res = await axiosInstance.get("/auth/check");

      const paired = await pairLegacyIdentity(res.data, axiosInstance, undefined, { onRecoveryKey: get().showRecoveryKey });
      set({ authUser: paired, unpairedUser: null, unpairedReason: null });
      get().connectSocket();
    } catch (error) {
      if (get().holdUnpaired(error)) return;
      console.log("Error in checkAuth:", error);
      set({ authUser: null, unpairedUser: null });
    } finally {
      set({ isCheckingAuth: false });
    }
  },

  signup: async (data) => {
    set({ isSigningUp: true });
    try {
      const res = await axiosInstance.post("/auth/signup", data);
      const paired = await pairLegacyIdentity(res.data, axiosInstance, data.password, { onRecoveryKey: get().showRecoveryKey });
      set({ authUser: paired, unpairedUser: null, unpairedReason: null });
      toast.success("Account created successfully");
      get().connectSocket();
    } catch (error) {
      if (get().holdUnpaired(error)) return;
      toast.error(error.response?.data?.message || error.message);
    } finally {
      set({ isSigningUp: false });
    }
  },

  login: async (data) => {
    set({ isLoggingIn: true });
    try {
      const res = await axiosInstance.post("/auth/login", data);
      const paired = await pairLegacyIdentity(res.data, axiosInstance, data.password, { onRecoveryKey: get().showRecoveryKey });
      set({ authUser: paired, unpairedUser: null, unpairedReason: null });
      toast.success("Logged in successfully");

      get().connectSocket();
    } catch (error) {
      if (get().holdUnpaired(error)) return;
      toast.error(error.response?.data?.message || error.message);
    } finally {
      set({ isLoggingIn: false });
    }
  },

  loginWithPasskey: async () => {
    let popup;
    set({ isLoggingIn: true });
    try {
      popup = openPasskeyWindow();
      const { options, origin } = await passkeyRequest("login/options");
      const response = await runPasskey(
        "authentication",
        options,
        origin,
        popup,
      );
      const user = await passkeyRequest("login/verify", { response });
      const paired = await pairLegacyIdentity(user, axiosInstance, undefined, { onRecoveryKey: get().showRecoveryKey });
      set({ authUser: paired, unpairedUser: null, unpairedReason: null });
      get().connectSocket();
      toast.success("Signed in with your passkey");
    } catch (error) {
      if (get().holdUnpaired(error)) return;
      toast.error(passkeyError(error));
    } finally {
      popup?.close();
      set({ isLoggingIn: false });
    }
  },

  logout: async () => {
    try {
      await axiosInstance.post("/auth/logout");
      get().clearSession();
      toast.success("Logged out successfully");
    } catch (error) {
      toast.error(error.response?.data?.message || error.message);
    }
  },

  updateProfile: async (data) => {
    set({ isUpdatingProfile: true });
    try {
      const res = await axiosInstance.put("/auth/update-profile", data);
      set({ authUser: res.data });
      toast.success("Profile updated successfully");
      return true;
    } catch (error) {
      console.log("error in update profile:", error);
      toast.error(error.response?.data?.message || error.message);
    } finally {
      set({ isUpdatingProfile: false });
    }
  },

  connectSocket: () => {
    const { authUser } = get();
    if (!authUser || get().socket) return;

    const socket = io(BASE_URL, {
      withCredentials: true,
    });

    set({ socket: socket });
    socket.on("sessionRevoked", () => {
      get().clearSession();
    });
    socket.on("connect_error", async (error) => {
      if (error.message === "Authentication required") {
        try {
          await axiosInstance.get("/auth/check");
        } catch (requestError) {
          if (requestError.response?.status === 401) get().clearSession();
        }
      }
    });

    socket.on("getOnlineUsers", (userIds) => {
      set({ onlineUsers: userIds });
    });
    socket.on("getPresence", (presence) => set({ presence }));
    socket.on("ownPresence", ({ presence }) => {
      if (get().authUser) set({ authUser: { ...get().authUser, presence } });
    });
  },
  disconnectSocket: () => {
    get().socket?.disconnect();
    set({ socket: null, onlineUsers: [], presence: {} });
  },
  setPresence: async (presence) => {
    const previous = get().authUser?.presence;
    set({ authUser: { ...get().authUser, presence } });
    try {
      await axiosInstance.put("/auth/presence", { presence });
    } catch {
      set({ authUser: { ...get().authUser, presence: previous } });
      toast.error("Could not change your status");
    }
  },
}));
