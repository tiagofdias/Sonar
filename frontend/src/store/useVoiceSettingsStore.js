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
  const { inputDeviceId, echoCancellation, noiseSuppression, autoGainControl } =
    useVoiceSettingsStore.getState();
  return {
    ...(inputDeviceId ? { deviceId: { exact: inputDeviceId } } : {}),
    echoCancellation,
    noiseSuppression,
    autoGainControl,
  };
}
