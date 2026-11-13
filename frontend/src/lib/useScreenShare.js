import { useCallback, useEffect, useRef, useState } from "react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "./axios";
import { applyAudioOutput } from "./audioOutput";
import { preferStereo } from "./sdpStereo";
import { useVoiceSettingsStore } from "../store/useVoiceSettingsStore";

// AV1 first; other codecs stay as a fallback for browsers that cannot do AV1.
export function preferAv1(codecs) {
  const isAv1 = (codec) => codec.mimeType.toLowerCase() === "video/av1";
  return [...codecs.filter(isAv1), ...codecs.filter((codec) => !isAv1(codec))];
}

const MAX_FPS = 30;
export const SCREEN_QUALITIES = {
  1080: { label: "1080p", width: 1920, height: 1080, bitrate: 4_000_000 },
  1440: { label: "1440p", width: 2560, height: 1440, bitrate: 7_000_000 },
  2160: { label: "4K", width: 3840, height: 2160, bitrate: 12_000_000 },
};

const newConnection = async () => {
  const { data } = await api.get("/voice/config");
  return new RTCPeerConnection({
    iceServers: data.iceServers,
    iceTransportPolicy: data.relayOnly || useVoiceSettingsStore.getState().relayOnly ? "relay" : "all",
  });
};

const applyPending = async (entry) => {
  for (const candidate of entry.pending.splice(0)) await entry.pc.addIceCandidate(candidate);
};

// Sends the local screen to each target and shows screens sent to us, each on
// its own encrypted peer connection so it can never disturb the voice path.
export function useScreenShare({ send, targets }) {
  const [sharing, setSharing] = useState(false);
  const [remotes, setRemotes] = useState({});
  // TV shares from the desktop app name their channel; screens have no label.
  const [labels, setLabels] = useState({});
  // TV shares wait here until we choose to join: sharer id -> channel name.
  const [available, setAvailable] = useState({});
  // Listen together: sharer id -> what is playing (music, heard straight away, no picture).
  const [music, setMusic] = useState({});
  const offers = useRef(new Map());
  const media = useRef(null);
  const bitrate = useRef(SCREEN_QUALITIES[1080].bitrate);
  const outgoing = useRef(new Map());
  const incoming = useRef(new Map());
  const sendRef = useRef(send);
  useEffect(() => { sendRef.current = send; });

  const closeOutgoing = useCallback((id, notify = true) => {
    const entry = outgoing.current.get(id);
    if (!entry) return;
    outgoing.current.delete(id);
    entry.pc.close();
    if (notify) Promise.resolve(sendRef.current(id, { type: "screen-stop" })).catch(() => {});
  }, []);
  const forgetOffer = useCallback((id) => {
    offers.current.delete(id);
    setAvailable((all) => {
      if (!(id in all)) return all;
      const rest = { ...all };
      delete rest[id];
      return rest;
    });
  }, []);
  const closeIncoming = useCallback((id) => {
    const entry = incoming.current.get(id);
    if (entry) { incoming.current.delete(id); entry.pc.close(); }
    setRemotes((all) => {
      if (!(id in all)) return all;
      const rest = { ...all };
      delete rest[id];
      return rest;
    });
    setLabels((all) => {
      if (!(id in all)) return all;
      const rest = { ...all };
      delete rest[id];
      return rest;
    });
    setMusic((all) => {
      if (!(id in all)) return all;
      const rest = { ...all };
      delete rest[id];
      return rest;
    });
  }, []);

  const stop = useCallback(() => {
    for (const id of [...outgoing.current.keys()]) closeOutgoing(id);
    media.current?.getTracks().forEach((track) => track.stop());
    media.current = null;
    setSharing(false);
  }, [closeOutgoing]);

  const offerTo = useCallback(async (id) => {
    const stream = media.current;
    if (!stream || outgoing.current.has(id)) return;
    const entry = { pc: null, pending: [] };
    outgoing.current.set(id, entry);
    try {
      entry.pc = await newConnection();
      if (outgoing.current.get(id) !== entry || media.current !== stream) { entry.pc.close(); return; }
      entry.pc.onicecandidate = ({ candidate }) => {
        if (candidate) Promise.resolve(sendRef.current(id, { type: "screen-ice", role: "sharer", candidate: candidate.toJSON() })).catch(() => {});
      };
      for (const track of stream.getTracks()) {
        const sender = entry.pc.addTrack(track, stream);
        if (track.kind === "video") {
          const transceiver = entry.pc.getTransceivers().find((item) => item.sender === sender);
          const codecs = RTCRtpSender.getCapabilities?.("video")?.codecs;
          if (codecs && transceiver?.setCodecPreferences) transceiver.setCodecPreferences(preferAv1(codecs));
          const parameters = sender.getParameters();
          // The browser lowers quality on its own when this PC cannot keep up.
          parameters.degradationPreference = "balanced";
          parameters.encodings = (parameters.encodings?.length ? parameters.encodings : [{}]).map((encoding) => ({ ...encoding, maxBitrate: bitrate.current, maxFramerate: MAX_FPS }));
          await sender.setParameters(parameters).catch(() => {});
        }
      }
      await entry.pc.setLocalDescription(await entry.pc.createOffer());
      await sendRef.current(id, { type: "screen-offer", sdp: entry.pc.localDescription.toJSON() });
    } catch {
      if (outgoing.current.get(id) === entry) { outgoing.current.delete(id); entry.pc?.close(); }
    }
  }, []);

  const start = useCallback(async (height = 1080) => {
    if (media.current) return;
    const quality = SCREEN_QUALITIES[height] || SCREEN_QUALITIES[1080];
    bitrate.current = quality.bitrate;
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: { width: { max: quality.width }, height: { max: quality.height }, frameRate: { max: MAX_FPS } },
        // A browser shares a tab's own sound or the whole PC's; a window's sound only where it supports it.
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, restrictOwnAudio: true },
        systemAudio: "include",
        windowAudio: "window",
      });
      const video = stream.getVideoTracks()[0];
      if (video) {
        video.contentHint = "detail";
        video.onended = () => stop();
      }
      media.current = stream;
      setSharing(true);
    } catch (error) {
      if (error.name !== "NotAllowedError" && error.name !== "AbortError") toast.error("Could not share your screen");
    }
  }, [stop]);

  const targetKey = targets.join(",");
  useEffect(() => {
    const wanted = targetKey ? targetKey.split(",") : [];
    for (const id of [...incoming.current.keys()]) if (!wanted.includes(id)) closeIncoming(id);
    for (const id of [...offers.current.keys()]) if (!wanted.includes(id)) forgetOffer(id);
    for (const id of [...outgoing.current.keys()]) if (!wanted.includes(id)) closeOutgoing(id, false);
    if (!wanted.length && media.current) stop();
    if (sharing) wanted.forEach(offerTo);
  }, [targetKey, sharing, offerTo, stop, closeIncoming, closeOutgoing, forgetOffer]);

  useEffect(() => () => {
    for (const entry of outgoing.current.values()) entry.pc?.close();
    for (const entry of incoming.current.values()) entry.pc?.close();
    media.current?.getTracks().forEach((track) => track.stop());
  }, []);

  const answer = useCallback(async (from, signal) => {
    closeIncoming(from);
    const entry = { pc: null, pending: [] };
    incoming.current.set(from, entry);
    const name = typeof signal.label === "string" && signal.label ? signal.label.slice(0, 80) : "";
    if (signal.music) setMusic((all) => ({ ...all, [from]: name || "Music" }));
    else if (signal.tv) setLabels((all) => ({ ...all, [from]: name || "TV" }));
    try {
      entry.pc = await newConnection();
      if (incoming.current.get(from) !== entry) return entry.pc.close();
      entry.pc.onicecandidate = ({ candidate }) => {
        if (candidate) Promise.resolve(sendRef.current(from, { type: "screen-ice", role: "viewer", candidate: candidate.toJSON() })).catch(() => {});
      };
      entry.pc.ontrack = ({ streams }) => {
        if (streams[0] && incoming.current.get(from) === entry) setRemotes((all) => ({ ...all, [from]: streams[0] }));
      };
      entry.pc.onconnectionstatechange = () => {
        if (entry.pc.connectionState === "failed" && incoming.current.get(from) === entry) closeIncoming(from);
      };
      await entry.pc.setRemoteDescription(signal.sdp);
      await applyPending(entry);
      // TV and music are sent in stereo; browsers only keep both channels when the answer asks for it.
      const made = await entry.pc.createAnswer();
      await entry.pc.setLocalDescription(signal.music || signal.tv ? { type: made.type, sdp: preferStereo(made.sdp) } : made);
      await sendRef.current(from, { type: "screen-answer", sdp: entry.pc.localDescription.toJSON() });
    } catch {
      closeIncoming(from);
    }
  }, [closeIncoming]);

  // Start watching a TV share that is waiting for us.
  const join = useCallback((from) => {
    const signal = offers.current.get(from);
    forgetOffer(from);
    if (signal) answer(from, signal);
  }, [answer, forgetOffer]);
  // Stop watching; the sharer offers it again, so it can be joined later.
  const leave = useCallback((from) => {
    closeIncoming(from);
    Promise.resolve(sendRef.current(from, { type: "screen-leave" })).catch(() => {});
  }, [closeIncoming]);

  const handle = useCallback(async (from, signal) => {
    try {
      if (signal.type === "screen-stop") { forgetOffer(from); return closeIncoming(from); }
      if (signal.type === "screen-leave") {
        // Someone stopped watching our share: offer it again so they can come back.
        if (outgoing.current.has(from)) { closeOutgoing(from, false); if (media.current) offerTo(from); }
        return;
      }
      if (signal.type === "screen-answer") {
        const entry = outgoing.current.get(from);
        if (!entry?.pc || entry.pc.signalingState !== "have-local-offer") return;
        await entry.pc.setRemoteDescription(signal.sdp);
        await applyPending(entry);
        return;
      }
      if (signal.type === "screen-ice") {
        const entry = (signal.role === "sharer" ? incoming : outgoing).current.get(from);
        if (!entry) return;
        if (entry.pc?.remoteDescription) await entry.pc.addIceCandidate(signal.candidate);
        else if (entry.pending.length < 128) entry.pending.push(signal.candidate);
        return;
      }
      if (signal.type === "screen-label") {
        const label = signal.label.slice(0, 80);
        if (incoming.current.has(from)) {
          setMusic((all) => (from in all ? { ...all, [from]: label } : all));
          setLabels((all) => (from in all ? { ...all, [from]: label } : all));
        }
        if (offers.current.has(from)) setAvailable((all) => ({ ...all, [from]: label }));
        return;
      }
      if (signal.type !== "screen-offer") return;
      closeIncoming(from);
      // TV is joined when you choose; screens open straight away.
      if (signal.tv) {
        offers.current.set(from, signal);
        setAvailable((all) => ({ ...all, [from]: typeof signal.label === "string" && signal.label ? signal.label.slice(0, 80) : "TV" }));
        return;
      }
      await answer(from, signal);
    } catch {
      if (signal.type === "screen-offer") closeIncoming(from);
    }
  }, [closeIncoming, closeOutgoing, offerTo, forgetOffer, answer]);

  return { supported: Boolean(navigator.mediaDevices?.getDisplayMedia), sharing, start, stop, remotes, labels, music, available, join, leave, handle };
}

export const applyScreenOutput = (player) =>
  applyAudioOutput(player, useVoiceSettingsStore.getState());
