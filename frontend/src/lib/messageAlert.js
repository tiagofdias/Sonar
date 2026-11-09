import toast from "react-hot-toast";
import { useAuthStore } from "../store/useAuthStore";
import { useVoiceSettingsStore } from "../store/useVoiceSettingsStore";
import { applyAudioOutput } from "./audioOutput";
import { usePreferencesStore } from "../store/usePreferencesStore";

const GAP_MS = 1200;
let lastSound = 0;

// A new message arrived somewhere the person is not looking. Busy means no sound.
export async function messageAlert(text) {
  if (!document.hidden) toast(text, { id: text });
  if (useAuthStore.getState().authUser?.presence === "busy") return;
  const now = Date.now();
  if (now - lastSound < GAP_MS) return;
  lastSound = now;
  const player = new Audio("/sounds/message.wav");
  player.volume = usePreferencesStore.getState().alertVolume;
  try {
    await applyAudioOutput(player, useVoiceSettingsStore.getState());
    await player.play();
  } catch { /* The browser blocks sound until the page has been used. */ }
}
