import { create } from "zustand";
export const useVoiceSettingsStore = create((set) => ({
  inputDeviceId: "",
  outputDeviceId: "",
  outputVolume: 0.75,
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true,
  relayOnly: false,
  update: (patch) => set(patch),
}));
export function audioConstraints() {
  const { inputDeviceId, echoCancellation, autoGainControl } =
    useVoiceSettingsStore.getState();
  return {
    ...(inputDeviceId ? { deviceId: { exact: inputDeviceId } } : {}),
    echoCancellation,
    // DeepFilterNet does this instead (see lib/noiseSuppression.js); running both muffles speech.
    noiseSuppression: false,
    autoGainControl,
  };
}

// The chosen microphone, or the default one when it was unplugged (or renamed) since it was chosen.
export async function openMicrophone() {
  try {
    return await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(), video: false });
  } catch (error) {
    if (!useVoiceSettingsStore.getState().inputDeviceId || !["OverconstrainedError", "NotFoundError"].includes(error.name)) throw error;
    useVoiceSettingsStore.setState({ inputDeviceId: "" });
    return navigator.mediaDevices.getUserMedia({ audio: audioConstraints(), video: false });
  }
}
