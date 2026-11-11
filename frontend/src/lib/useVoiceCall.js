import { usePreferencesStore } from "../store/usePreferencesStore";
import { useEffect, useRef, useState, useCallback } from "react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "./axios";
import { applyAudioOutput } from "./audioOutput";
import { useScreenShare } from "./useScreenShare";
import { denoisedStream } from "./noiseSuppression";
import { useChatStore } from "../store/useChatStore";
import { refreshVoiceConfiguration } from "./voiceConfigRefresh";
import {
  audioConstraints,
  useVoiceSettingsStore,
} from "../store/useVoiceSettingsStore";
const mediaError = (error) =>
  error.name === "NotAllowedError"
    ? "Microphone access was denied. Allow it in your browser and try again."
    : error.name === "NotFoundError"
      ? "No microphone was found."
      : error.name === "OverconstrainedError"
        ? "The selected microphone is unavailable. Choose another in Voice settings."
        : error.message;
// Loops the ringtone on the chosen speakers until the call is answered or ends.
function useRingtone(level) {
  useEffect(() => {
    if (!level) return undefined;
    const player = new Audio("/sounds/ringtone.wav");
    player.loop = true;
    player.volume = Math.min(1, level * (usePreferencesStore.getState().alertVolume / 0.8));
    const play = () => player.play().catch(() => {});
    // Browsers block sound on a page nobody has touched yet: retry on first input.
    const retry = () => { play(); stopRetry(); };
    const stopRetry = () => ["pointerdown", "keydown"].forEach((name) => window.removeEventListener(name, retry));
    applyAudioOutput(player, useVoiceSettingsStore.getState()).catch(() => {}).finally(() => {
      if (player.dataset.stopped) return;
      player.play().catch(() => ["pointerdown", "keydown"].forEach((name) => window.addEventListener(name, retry)));
    });
    return () => {
      player.dataset.stopped = "1";
      stopRetry();
      player.pause();
      player.removeAttribute("src");
      player.load();
    };
  }, [level]);
}
export function useVoiceCall(socket, inRoom = false) {
  // Reached the other person, and whether they picked up: decides "missed call".
  const rang = useRef(false), answered = useRef(false);
  const screenRef = useRef(null);
  const [waiting, setWaiting] = useState(null);
  const waitingRef = useRef(null);
  const setWait = (value) => { waitingRef.current = value; setWaiting(value); };
  const cleaned = useRef(null);
  const [call, setCall] = useState(null),
    [muted, setMuted] = useState(false),
    [deafened, setDeafened] = useState(false),
    [playbackBlocked, setPlaybackBlocked] = useState(false);
  const state = useRef(null),
    pc = useRef(null),
    stream = useRef(null),
    audio = useRef(null),
    pending = useRef([]),
    local = useRef([]),
    ready = useRef(false),
    disconnectedTimer = useRef(null),
    stopConfigRefresh = useRef(null),
    muteRef = useRef(false),
    deafRef = useRef(false);
  const update = (value) => {
    state.current = value;
    setCall(value);
  };
  const signal = (to, payload, confirm = false) =>
    new Promise((resolve, reject) => {
      if (!socket?.connected) {
        if (confirm)
          reject(new Error("Connection lost. Reconnect before calling."));
        else resolve();
        return;
      }
      if (!confirm) {
        socket.emit("call:signal", { to, signal: payload });
        resolve();
        return;
      }
      socket
        .timeout(5000)
        .emit("call:signal", { to, signal: payload }, (error, result) =>
          error || result?.error
            ? reject(new Error(result?.error || "Call signaling timed out"))
            : resolve(),
        );
    });
  const cleanup = () => {
    const finished = state.current;
    if (finished?.status === "calling" && rang.current && !answered.current)
      useChatStore.getState().sendMissedCall(finished.peer);
    rang.current = false;
    answered.current = false;
    update(null);
    ready.current = false;
    pending.current = [];
    local.current = [];
    clearTimeout(disconnectedTimer.current);
    stopConfigRefresh.current?.();
    stopConfigRefresh.current = null;
    const connection = pc.current;
    pc.current = null;
    if (connection) {
      connection.onconnectionstatechange = null;
      connection.close();
    }
    stream.current?.getTracks().forEach((track) => {
      track.onended = null;
      track.stop();
    });
    stream.current = null;
    cleaned.current?.close();
    cleaned.current = null;
    if (audio.current) {
      audio.current.pause();
      audio.current.srcObject = null;
      audio.current = null;
    }
    muteRef.current = false;
    deafRef.current = false;
    setMuted(false);
    setDeafened(false);
    setPlaybackBlocked(false);
  };
  const hangup = () => {
    const current = state.current;
    if (current) signal(current.peer, { type: "hangup", callId: current.id });
    cleanup();
  };
  const valid = (id) => state.current?.id === id;
  const playOutput = async () => {
    const player = audio.current;
    if (!player) return;
    await applyAudioOutput(player, useVoiceSettingsStore.getState());
    if (audio.current !== player || !state.current) return;
    await player.play();
    if (audio.current === player) setPlaybackBlocked(false);
  };
  const flushLocal = (current) => {
    ready.current = true;
    for (const candidate of local.current.splice(0))
      signal(current.peer, { type: "ice", callId: current.id, candidate });
  };
  const prepare = async (current) => {
    const { data: config } = await api.get("/voice/config");
    if (!valid(current.id)) throw new Error("Call ended");
    const relayOnly =
      config.relayOnly || useVoiceSettingsStore.getState().relayOnly;
    if (relayOnly && !config.turnConfigured)
      throw new Error("Relay-only calls need a configured TURN server.");
    if (!navigator.mediaDevices?.getUserMedia)
      throw new Error(
        "Voice calls need microphone access over HTTPS or localhost.",
      );
    const media = await navigator.mediaDevices.getUserMedia({
      audio: audioConstraints(),
      video: false,
    });
    if (!valid(current.id)) {
      media.getTracks().forEach((track) => track.stop());
      throw new Error("Call ended");
    }
    stream.current = media;
    media.getAudioTracks().forEach((track) => {
      track.enabled = !muteRef.current && !deafRef.current;
      track.onended = () => {
        if (!valid(current.id) || stream.current !== media) return;
        toast.error("Microphone disconnected. Choose another device and call again.");
        hangup();
      };
    });
    const connection = new RTCPeerConnection({
      iceServers: config.iceServers,
      iceTransportPolicy: relayOnly ? "relay" : "all",
    });
    pc.current = connection;
    stopConfigRefresh.current = refreshVoiceConfiguration(
      config,
      async () => (await api.get("/voice/config")).data,
      (fresh) => {
        if (!valid(current.id) || pc.current !== connection) return;
        if ((relayOnly || fresh.relayOnly) && !fresh.turnConfigured)
          throw new Error("TURN unavailable");
        connection.setConfiguration({ ...connection.getConfiguration(),
          iceServers: fresh.iceServers,
          iceTransportPolicy: relayOnly || fresh.relayOnly ? "relay" : "all" });
      },
    );
    // Ring straight away with the microphone as it is; the noise filter can take
    // seconds to download and start, so its audio replaces the track once ready.
    const raw = media.getAudioTracks()[0];
    const sender = connection.addTrack(raw, media);
    denoisedStream(media).then((clean) => {
      if (!valid(current.id) || pc.current !== connection) return clean.close();
      cleaned.current = clean;
      const track = clean.stream.getAudioTracks()[0];
      if (track && track !== raw) sender.replaceTrack(track).catch(() => {});
    }).catch(() => {}); // Keep the plain microphone if the filter cannot start.
    update({ ...state.current, relayOnly });
    connection.onicecandidate = (event) => {
      if (!valid(current.id) || !event.candidate) return;
      const candidate = event.candidate.toJSON();
      if (ready.current)
        signal(current.peer, { type: "ice", callId: current.id, candidate });
      else local.current.push(candidate);
    };
    connection.ontrack = (event) => {
      if (!valid(current.id)) return;
      audio.current ||= new Audio();
      audio.current.muted = deafRef.current;
      audio.current.srcObject =
        event.streams[0] || new MediaStream([event.track]);
      playOutput().catch(() => {
        if (valid(current.id)) setPlaybackBlocked(true);
      });
    };
    connection.onconnectionstatechange = () => {
      if (!valid(current.id) || pc.current !== connection) return;
      clearTimeout(disconnectedTimer.current);
      if (connection.connectionState === "connected")
        update({
          ...state.current,
          status: "connected",
          startedAt: state.current.startedAt || Date.now(),
        });
      if (connection.connectionState === "disconnected") {
        update({ ...state.current, status: "reconnecting" });
        disconnectedTimer.current = setTimeout(() => {
          if (valid(current.id)) {
            toast.error("Voice connection lost");
            hangup();
          }
        }, 10000);
      }
      if (connection.connectionState === "failed") {
        toast.error("Voice could not connect. This network may require TURN.");
        hangup();
      }
    };
    return connection;
  };
  const start = async (peer) => {
    if (state.current) return;
    const current = {
      id: crypto.randomUUID(),
      peer: peer._id,
      name: peer.fullName,
      status: "calling",
    };
    update(current);
    try {
      const connection = await prepare(current);
      await connection.setLocalDescription(await connection.createOffer());
      if (!valid(current.id)) return;
      await signal(
        current.peer,
        {
          type: "offer",
          callId: current.id,
          sdp: connection.localDescription.toJSON(),
        },
        true,
      );
      if (valid(current.id)) { rang.current = true; flushLocal(current); }
    } catch (error) {
      if (valid(current.id)) {
        toast.error(mediaError(error));
        hangup();
      }
    }
  };
  const accept = async () => {
    const current = state.current;
    if (current?.status !== "incoming") return;
    update({ ...current, status: "connecting" });
    try {
      const connection = await prepare(current);
      await connection.setRemoteDescription(current.sdp);
      for (const candidate of pending.current.splice(0))
        await connection.addIceCandidate(candidate);
      await connection.setLocalDescription(await connection.createAnswer());
      if (!valid(current.id)) return;
      await signal(
        current.peer,
        {
          type: "answer",
          callId: current.id,
          sdp: connection.localDescription.toJSON(),
        },
        true,
      );
      if (valid(current.id)) flushLocal(current);
    } catch (error) {
      if (valid(current.id)) {
        toast.error(mediaError(error));
        hangup();
      }
    }
  };
  useEffect(() => {
    if (!socket) return;
    const receive = async ({ from, signal: payload }) => {
      try {
        if (payload.type === "offer") {
          if (state.current) {
            if (["connected", "reconnecting"].includes(state.current.status) && !waitingRef.current)
              setWait({ id: payload.callId, peer: from, sdp: payload.sdp, pending: [] });
            return;
          }
          update({
            id: payload.callId,
            peer: from,
            status: "incoming",
            sdp: payload.sdp,
          });
          return;
        }
        if (waitingRef.current?.id === payload.callId && waitingRef.current.peer === from) {
          if (payload.type === "ice" && waitingRef.current.pending.length < 128) waitingRef.current.pending.push(payload.candidate);
          return;
        }
        if (!valid(payload.callId) || state.current.peer !== from) return;
        if (payload.type?.startsWith("screen-")) return screenRef.current.handle(from, payload);
        if (
          payload.type === "answer" &&
          pc.current?.signalingState === "have-local-offer"
        ) {
          answered.current = true;
          const connection = pc.current;
          await connection.setRemoteDescription(payload.sdp);
          for (const candidate of pending.current.splice(0))
            await connection.addIceCandidate(candidate);
        }
        if (payload.type === "ice") {
          if (pc.current?.remoteDescription)
            await pc.current.addIceCandidate(payload.candidate);
          else if (pending.current.length < 128)
            pending.current.push(payload.candidate);
        }
      } catch {
        if (valid(payload.callId)) {
          toast.error("Could not establish voice connection");
          hangup();
        }
      }
    };
    const ended = ({ callId, reason }) => {
      if (waitingRef.current?.id === callId) setWait(null);
      if (valid(callId)) {
        cleanup();
        if (reason === "unanswered") toast("No answer. Try again later.");
      }
    };
    const disconnected = () => cleanup();
    socket.on("call:signal", receive);
    socket.on("call:ended", ended);
    socket.on("disconnect", disconnected);
    return () => {
      socket.off("call:signal", receive);
      socket.off("call:ended", ended);
      socket.off("disconnect", disconnected);
      hangup();
    };
  }, [socket]);
  useEffect(() => {
    if (!call || call.startedAt) return;
    const timer = setTimeout(hangup, 60000);
    return () => clearTimeout(timer);
  }, [call?.id, call?.startedAt]);
  const updateTracks = () =>
    stream.current?.getAudioTracks().forEach((track) => {
      track.enabled = !muteRef.current && !deafRef.current;
    });
  const toggleMute = () => {
    muteRef.current = !muteRef.current;
    setMuted(muteRef.current);
    updateTracks();
  };
  const toggleDeafen = () => {
    deafRef.current = !deafRef.current;
    setDeafened(deafRef.current);
    if (audio.current) audio.current.muted = deafRef.current;
    updateTracks();
  };
  useEffect(
    () =>
      useVoiceSettingsStore.subscribe((next, previous) => {
        if (
          !audio.current ||
          (next.outputDeviceId === previous.outputDeviceId &&
            next.outputVolume === previous.outputVolume)
        )
          return;
        if (next.outputDeviceId !== previous.outputDeviceId)
          audio.current.pause();
        playOutput().catch(() => {
          if (audio.current) {
            setPlaybackBlocked(true);
            toast.error(
              "Selected speakers are unavailable. Choose another output in Voice settings.",
            );
          }
        });
      }),
    [],
  );
  const resumeAudio = () =>
    playOutput().catch(() =>
      toast.error(
        "Check your selected speakers and allow audio playback in your browser.",
      ),
    );
  // A second call rings softly so it never drowns out the conversation.
  useRingtone(call?.status === "incoming" ? (inRoom ? 0.35 : 1) : waiting ? 0.35 : 0);
  useEffect(() => {
    if (call || !waiting) return;
    const next = waiting;
    setWait(null);
    pending.current = next.pending;
    update({ id: next.id, peer: next.peer, status: "incoming", sdp: next.sdp });
  }, [call, waiting]);
  const swapCall = async () => {
    const next = waitingRef.current;
    if (!next) return;
    setWait(null);
    hangup();
    pending.current = next.pending;
    update({ id: next.id, peer: next.peer, status: "incoming", sdp: next.sdp });
    await accept();
  };
  const declineWaiting = () => {
    const next = waitingRef.current;
    if (!next) return;
    setWait(null);
    signal(next.peer, { type: "hangup", callId: next.id });
  };
  const screen = useScreenShare({
    send: (to, payload) => signal(to, { ...payload, callId: state.current?.id }),
    targets: call?.status === "connected" || call?.status === "reconnecting" ? [call.peer] : [],
  });
  screenRef.current = screen;
  const micStream = useCallback(() => stream.current, []);
  return {
    call,
    start,
    accept,
    hangup,
    muted,
    deafened,
    toggleMute,
    toggleDeafen,
    playbackBlocked,
    resumeAudio,
    micStream,
    screen,
    waiting,
    swapCall,
    declineWaiting,
  };
}
