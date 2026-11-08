import { useEffect, useRef, useState } from "react";
import { Headphones, Mic, ShieldCheck, Square } from "lucide-react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";
import {
  useVoiceSettingsStore,
  audioConstraints,
} from "../store/useVoiceSettingsStore";
import SpeakerSettings from "./SpeakerSettings";
import { testVoiceRelay } from "../lib/voiceRelayTest";
export default function VoiceSettings() {
  const settings = useVoiceSettingsStore();
  const [devices, setDevices] = useState([]),
    [testing, setTesting] = useState(false),
    [level, setLevel] = useState(0),
    [starting, setStarting] = useState(false),
    [config, setConfig] = useState(null),
    [networkError, setNetworkError] = useState(""),
    [relayTest, setRelayTest] = useState("idle");
  const media = useRef(null),
    context = useRef(null),
    frame = useRef(null),
    epoch = useRef(0),
    relayProbe = useRef(null),
    relayEpoch = useRef(0);
  const release = () => {
    epoch.current++;
    cancelAnimationFrame(frame.current);
    media.current?.getTracks().forEach((track) => track.stop());
    media.current = null;
    context.current?.close();
    context.current = null;
  };
  const stop = () => {
    release();
    setTesting(false);
    setStarting(false);
    setLevel(0);
  };
  useEffect(() => {
    let active = true;
    api
      .get("/voice/config")
      .then(({ data }) => {
        if (active) setConfig(data);
      })
      .catch((error) => {
        if (active)
          setNetworkError(
            error.response?.data?.message ||
              "Could not check voice connectivity",
          );
      });
    return () => {
      active = false;
      relayEpoch.current++;
      relayProbe.current?.cancel();
      release();
    };
  }, []);
  const test = async () => {
    if (testing || starting) {
      stop();
      return;
    }
    setStarting(true);
    const token = ++epoch.current;
    try {
      if (!navigator.mediaDevices?.getUserMedia)
        throw new Error(
          "Microphone access requires HTTPS or localhost and a supported browser.",
        );
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: audioConstraints(),
        video: false,
      });
      if (token !== epoch.current) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }
      media.current = stream;
      const inputs = await navigator.mediaDevices.enumerateDevices();
      if (token !== epoch.current) return;
      setDevices(
        inputs.filter(
          (device) =>
            device.kind === "audioinput" && device.deviceId !== "default",
        ),
      );
      const audioContext = new AudioContext();
      context.current = audioContext;
      await audioContext.resume();
      if (token !== epoch.current) return;
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 256;
      audioContext.createMediaStreamSource(stream).connect(analyser);
      const data = new Float32Array(analyser.fftSize);
      let previous = 0;
      const sample = (now) => {
        if (token !== epoch.current) return;
        if (now - previous > 80) {
          analyser.getFloatTimeDomainData(data);
          setLevel(
            Math.min(
              100,
              Math.sqrt(
                data.reduce((sum, value) => sum + value * value, 0) /
                  data.length,
              ) * 400,
            ),
          );
          previous = now;
        }
        frame.current = requestAnimationFrame(sample);
      };
      frame.current = requestAnimationFrame(sample);
      setTesting(true);
    } catch (error) {
      if (token === epoch.current) {
        stop();
        toast.error(
          error.name === "NotAllowedError"
            ? "Allow microphone access in your browser to test it."
            : error.message,
        );
      }
    } finally {
      if (token === epoch.current) setStarting(false);
    }
  };
  const checkRelay = async () => {
    const token = ++relayEpoch.current;
    setRelayTest("checking");
    try {
      const { data } = await api.get("/voice/config");
      if (token !== relayEpoch.current) return;
      if (!data.turnConfigured) throw new Error("Relay unavailable");
      setConfig(data);
      const probe = testVoiceRelay(data.iceServers);
      relayProbe.current = probe;
      const result = await probe.promise;
      if (token === relayEpoch.current) {
        relayProbe.current = null;
        setRelayTest(result);
      }
    } catch {
      if (token === relayEpoch.current) setRelayTest("unavailable");
    }
  };
  const change = (patch) => {
    if (testing || starting) stop();
    settings.update(patch);
  };
  return (
    <>
      <span className="eyebrow">FIND YOUR VOICE</span>
      <h3>Voice & audio</h3>
      <p>
        Check your microphone and speakers. Microphone changes apply to your
        next call; speaker changes apply immediately.
      </p>
      <section className="security-card">
        <div className="security-card-heading">
          <span className="security-icon">
            <Mic size={20} />
          </span>
          <div>
            <h4>Your microphone</h4>
            <p>Test your input locally. Nothing is recorded or uploaded.</p>
          </div>
        </div>
        <label className="dialog-field">
          Input device
          <select
            aria-label="Input device"
            value={settings.inputDeviceId}
            onChange={(event) => change({ inputDeviceId: event.target.value })}
          >
            <option value="">System default</option>
            {settings.inputDeviceId &&
              !devices.some(
                (device) => device.deviceId === settings.inputDeviceId,
              ) && (
                <option value={settings.inputDeviceId}>
                  Previously selected microphone
                </option>
              )}
            {devices.map((device, index) => (
              <option key={device.deviceId} value={device.deviceId}>
                {device.label || `Microphone ${index + 1}`}
              </option>
            ))}
          </select>
          <small>Start a microphone test to show available device names.</small>
        </label>
        <div
          className="microphone-meter"
          role="meter"
          aria-label="Microphone input level"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(level)}
        >
          <span style={{ width: `${level}%` }} />
        </div>
        <div className="microphone-test-row">
          <small>
            {testing
              ? "Listening locally. Speak to check the level."
              : "Microphone test is off."}
          </small>
          <button className="secondary" onClick={test}>
            {testing || starting ? <Square size={14} /> : <Mic size={14} />}{" "}
            {testing || starting ? "Stop microphone test" : "Test microphone"}
          </button>
        </div>
        <div className="audio-options">
          {[
            ["echoCancellation", "Echo cancellation"],
            ["noiseSuppression", "Noise suppression"],
            ["autoGainControl", "Automatic input volume"],
          ].map(([key, label]) => (
            <label className="settings-checkbox" key={key}>
              <input
                type="checkbox"
                checked={settings[key]}
                onChange={(event) => change({ [key]: event.target.checked })}
              />
              {label}
            </label>
          ))}
        </div>
      </section>
      <SpeakerSettings refreshToken={devices.length} />
      <section className="security-card">
        <div className="security-card-heading">
          <span className="security-icon">
            <ShieldCheck size={20} />
          </span>
          <div>
            <h4>Call connectivity</h4>
            <p>
              {networkError ||
                (config
                  ? config.turnConfigured
                    ? "An internet relay is configured."
                    : "Direct connections only. Internet calls may need a relay."
                  : "Checking voice configuration...")}
            </p>
          </div>
        </div>
        <button className="secondary" onClick={checkRelay}
          disabled={!config?.turnConfigured || relayTest === "checking"}>
          {relayTest === "checking" ? "Testing relay?" : "Test relay connection"}
        </button>
        <p className="settings-caption" role="status">
          {relayTest === "available" && "Relay reached. A call may still fail if another participant cannot connect."}
          {relayTest === "unavailable" && "Could not reach the relay from this network. Check the server and firewall settings."}
          {relayTest === "idle" && "This contacts the configured relay without using your microphone or placing a call."}
        </p>
        <label className="settings-checkbox">
          <input
            type="checkbox"
            checked={Boolean(config?.relayOnly || settings.relayOnly)}
            disabled={!config?.turnConfigured || config?.relayOnly}
            onChange={(event) =>
              settings.update({ relayOnly: event.target.checked })
            }
          />
          Use relay-only connections
        </label>
        <p className="settings-caption">
          Relay-only mode keeps your network address from the other participant.
          It requires an operator-configured TURN server. Audio stays encrypted
          in transit.
        </p>
      </section>
      <div className="honest-note">
        <Headphones size={16} />
        <p>
          Direct calls are available from the phone button in a conversation.
          Community voice rooms appear in the community sidebar and support up
          to six people. Sonar has no call recording or audio storage.
        </p>
      </div>
    </>
  );
}
