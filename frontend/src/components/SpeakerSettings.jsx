import { useEffect, useRef, useState } from "react";
import { RefreshCw, Speaker, Square, Volume2 } from "lucide-react";
import toast from "react-hot-toast";
import { useVoiceSettingsStore } from "../store/useVoiceSettingsStore";
import {
  applyAudioOutput,
  speakerTestBlob,
  supportsOutputSelection,
} from "../lib/audioOutput";
export default function SpeakerSettings({ refreshToken }) {
  const settings = useVoiceSettingsStore();
  const [devices, setDevices] = useState([]),
    [testing, setTesting] = useState(false),
    [choosing, setChoosing] = useState(false);
  const player = useRef(null),
    url = useRef(null),
    epoch = useRef(0);
  const release = () => {
    epoch.current++;
    if (player.current) {
      player.current.pause();
      player.current.removeAttribute("src");
      player.current.load();
      player.current = null;
    }
    if (url.current) {
      URL.revokeObjectURL(url.current);
      url.current = null;
    }
  };
  const stop = () => {
    release();
    setTesting(false);
  };
  const refresh = async () => {
    if (!navigator.mediaDevices?.enumerateDevices) return;
    try {
      const all = await navigator.mediaDevices.enumerateDevices();
      setDevices(
        all.filter(
          (device) =>
            device.kind === "audiooutput" && device.deviceId !== "default",
        ),
      );
    } catch {
      toast.error(
        "Could not list speakers. You can still use your system default.",
      );
    }
  };
  useEffect(() => {
    refresh();
    const changed = () => refresh();
    navigator.mediaDevices?.addEventListener?.("devicechange", changed);
    return () =>
      navigator.mediaDevices?.removeEventListener?.("devicechange", changed);
  }, [refreshToken]);
  useEffect(() => () => release(), []);
  const changeOutput = (value) => {
    stop();
    settings.update({ outputDeviceId: value });
  };
  const choose = async () => {
    if (!navigator.mediaDevices?.selectAudioOutput) {
      await refresh();
      return;
    }
    setChoosing(true);
    try {
      const device = await navigator.mediaDevices.selectAudioOutput();
      stop();
      await refresh();
      setDevices((list) =>
        list.some((item) => item.deviceId === device.deviceId)
          ? list
          : [...list, device],
      );
      settings.update({ outputDeviceId: device.deviceId });
    } catch (error) {
      if (error.name !== "NotAllowedError")
        toast.error(
          "Could not select those speakers. Choose System default or check your browser permissions.",
        );
    } finally {
      setChoosing(false);
    }
  };
  const test = async () => {
    if (testing) {
      stop();
      return;
    }
    release();
    const token = epoch.current;
    setTesting(true);
    try {
      const element = new Audio();
      player.current = element;
      url.current = URL.createObjectURL(speakerTestBlob());
      element.src = url.current;
      element.onended = () => {
        if (token === epoch.current) stop();
      };
      element.onerror = () => {
        if (token === epoch.current) {
          stop();
          toast.error(
            "The speaker test could not play. Check your browser sound settings.",
          );
        }
      };
      await applyAudioOutput(element, useVoiceSettingsStore.getState());
      if (token !== epoch.current) return;
      await element.play();
    } catch (error) {
      if (token === epoch.current) {
        stop();
        toast.error(
          error.name === "NotFoundError"
            ? "Selected speakers are unavailable. Choose another output."
            : "Could not play through those speakers. Choose System default or allow audio playback.",
        );
      }
    }
  };
  const volume = (event) => {
    const value = Number(event.target.value) / 100;
    settings.update({ outputVolume: value });
    if (player.current) player.current.volume = value;
  };
  const supported = supportsOutputSelection();
  return (
    <section className="security-card speaker-settings">
      <div className="security-card-heading">
        <span className="security-icon">
          <Speaker size={20} />
        </span>
        <div>
          <h4>Your speakers</h4>
          <p>Choose where you hear calls and play a short test sound.</p>
        </div>
      </div>
      <label className="dialog-field">
        Output device
        <select
          aria-label="Output device"
          value={settings.outputDeviceId}
          disabled={!supported}
          onChange={(event) => changeOutput(event.target.value)}
        >
          <option value="">System default</option>
          {settings.outputDeviceId &&
            !devices.some(
              (device) => device.deviceId === settings.outputDeviceId,
            ) && (
              <option value={settings.outputDeviceId}>
                Previously selected speakers
              </option>
            )}
          {devices.map((device, index) => (
            <option value={device.deviceId} key={device.deviceId}>
              {device.label || `Audio output ${index + 1}`}
            </option>
          ))}
        </select>
        <small>
          {supported
            ? "Available outputs depend on browser permission. Refresh after enabling your microphone if needed."
            : "This browser uses your system output. Change speakers in your device settings."}
        </small>
      </label>
      <div className="speaker-controls">
        <label className="output-volume-label" htmlFor="speaker-volume">
          Output volume{" "}
          <output htmlFor="speaker-volume">
            {Math.round(settings.outputVolume * 100)}%
          </output>
        </label>
        <input
          id="speaker-volume"
          aria-label="Output volume"
          type="range"
          min="0"
          max="100"
          value={Math.round(settings.outputVolume * 100)}
          onChange={volume}
        />
      </div>
      <div className="speaker-test-actions">
        <button type="button" className="secondary" onClick={test}>
          {testing ? <Square size={14} /> : <Volume2 size={15} />}{" "}
          {testing ? "Stop speaker test" : "Test speakers"}
        </button>
        {supported && (
          <button
            type="button"
            className="text-button"
            disabled={choosing}
            onClick={choose}
          >
            {navigator.mediaDevices?.selectAudioOutput ? (
              <Speaker size={14} />
            ) : (
              <RefreshCw size={14} />
            )}{" "}
            {choosing
              ? "Choosing..."
              : navigator.mediaDevices?.selectAudioOutput
                ? "Choose speakers"
                : "Refresh devices"}
          </button>
        )}
      </div>
      <p className="passkey-device-note">
        The test sound is generated on your device. It does not use your
        microphone. Speaker selection and volume also apply to active calls.
      </p>
    </section>
  );
}
