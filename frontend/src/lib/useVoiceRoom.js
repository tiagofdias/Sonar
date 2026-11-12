import { useEffect, useRef, useState, useCallback } from "react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "./axios";
import {
  openMicrophone,
  useVoiceSettingsStore,
} from "../store/useVoiceSettingsStore";
import { applyAudioOutput } from "./audioOutput";
import { useScreenShare } from "./useScreenShare";
import { denoisedStream } from "./noiseSuppression";
import { refreshVoiceConfiguration } from "./voiceConfigRefresh";
export function useVoiceRoom(socket) {
  const screenRef = useRef(null);
  const cleaned = useRef(null);
  const [room, setRoom] = useState(null),
    [rooms, setRooms] = useState({}),
    [connections, setConnections] = useState({}),
    [muted, setMuted] = useState(false),
    [deafened, setDeafened] = useState(false),
    [playbackBlocked, setPlaybackBlocked] = useState(false);
  const current = useRef(null),
    media = useRef(null),
    peers = useRef(new Map()),
    config = useRef(null),
    stopConfigRefresh = useRef(null),
    mute = useRef(false),
    deaf = useRef(false),
    early = useRef([]);
  const update = (value) => {
    current.current = value;
    setRoom(value);
  };
  const request = (event, payload) =>
    new Promise((resolve, reject) => {
      if (!socket?.connected)
        return reject(new Error("Reconnect before joining voice."));
      socket
        .timeout(5000)
        .emit(event, payload, (error, result) =>
          error || result?.error
            ? reject(new Error(result?.error || "Voice room request timed out"))
            : resolve(result),
        );
    });
  const closePeer = (id) => {
    const peer = peers.current.get(id);
    if (!peer) return;
    peers.current.delete(id);
    peer.pc.onconnectionstatechange = null;
    peer.pc.close();
    if (peer.audio) {
      peer.audio.pause();
      peer.audio.srcObject = null;
    }
    setConnections((value) => {
      const next = { ...value };
      delete next[id];
      return next;
    });
  };
  const cleanup = () => {
    update(null);
    for (const id of [...peers.current.keys()]) closePeer(id);
    media.current?.getTracks().forEach((track) => {
      track.onended = null;
      track.stop();
    });
    media.current = null;
    cleaned.current?.close();
    cleaned.current = null;
    stopConfigRefresh.current?.();
    stopConfigRefresh.current = null;
    config.current = null;
    early.current = [];
    mute.current = false;
    deaf.current = false;
    setMuted(false);
    setDeafened(false);
    setPlaybackBlocked(false);
  };
  const leave = () => {
    if (current.current?.sessionId && socket?.connected)
      request("voice:room-leave", { sessionId: current.current.sessionId }).catch(() => {});
    cleanup();
  };
  const send = (participant, signal) => {
    const active = current.current;
    return request("voice:room-signal", {
      communityId: active.communityId,
      channelId: active.channelId,
      sessionId: active.sessionId,
      to: participant.userId,
      toSession: participant.sessionId,
      signal,
    });
  };
  const play = async (peer) => {
    const player = peer.audio;
    if (!player) return;
    await applyAudioOutput(player, useVoiceSettingsStore.getState());
    if (!current.current || peers.current.get(peer.userId) !== peer) return;
    await player.play();
  };
  const flush = (peer) => {
    peer.ready = true;
    for (const candidate of peer.local.splice(0))
      send(peer, { type: "ice", candidate }).catch(() => {});
  };
  const makePeer = (participant) => {
    const existing = peers.current.get(participant.userId);
    if (existing?.sessionId === participant.sessionId) return existing;
    if (existing) closePeer(participant.userId);
    if (!media.current || !config.current)
      throw new Error("Room capture is unavailable");
    const pc = new RTCPeerConnection(config.current),
      peer = {
        ...participant,
        pc,
        audio: null,
        pending: [],
        local: [],
        ready: false,
      };
    peers.current.set(peer.userId, peer);
    const outgoing = cleaned.current?.stream || media.current;
    outgoing.getTracks().forEach((track) => pc.addTrack(track, outgoing));
    setConnections((value) => ({ ...value, [peer.userId]: "connecting" }));
    pc.onicecandidate = (event) => {
      if (!event.candidate || peers.current.get(peer.userId) !== peer) return;
      const candidate = event.candidate.toJSON();
      if (peer.ready) send(peer, { type: "ice", candidate }).catch(() => {});
      else peer.local.push(candidate);
    };
    pc.ontrack = (event) => {
      if (peers.current.get(peer.userId) !== peer) return;
      peer.audio ||= new Audio();
      peer.audio.muted = deaf.current;
      peer.audio.srcObject = event.streams[0] || new MediaStream([event.track]);
      play(peer).catch(() => {
        if (peers.current.get(peer.userId) === peer) setPlaybackBlocked(true);
      });
    };
    pc.onconnectionstatechange = () => {
      if (peers.current.get(peer.userId) !== peer) return;
      setConnections((value) => ({
        ...value,
        [peer.userId]: pc.connectionState,
      }));
      if (pc.connectionState === "failed") {
        peer.audio?.pause();
        toast.error("A room connection failed. Leave and rejoin to retry.");
      }
    };
    return peer;
  };
  const receive = async (event) => {
    const active = current.current;
    if (
      !active ||
      event.communityId !== active.communityId ||
      event.channelId !== active.channelId
    )
      return;
    if (!active.sessionId) {
      if (early.current.length < 128) early.current.push(event);
      return;
    }
    if (event.toSession !== active.sessionId) return;
    const participant = active.participants.find(
      (person) =>
        person.userId === event.from && person.sessionId === event.sessionId,
    );
    if (!participant) return;
    if (event.signal.type?.startsWith("screen-")) return screenRef.current.handle(event.from, event.signal);
    try {
      const peer = makePeer(participant),
        signal = event.signal;
      if (signal.type === "offer") {
        await peer.pc.setRemoteDescription(signal.sdp);
        for (const candidate of peer.pending.splice(0))
          await peer.pc.addIceCandidate(candidate);
        await peer.pc.setLocalDescription(await peer.pc.createAnswer());
        if (peers.current.get(peer.userId) !== peer) return;
        await send(peer, {
          type: "answer",
          sdp: peer.pc.localDescription.toJSON(),
        });
        flush(peer);
      } else if (
        signal.type === "answer" &&
        peer.pc.signalingState === "have-local-offer"
      ) {
        await peer.pc.setRemoteDescription(signal.sdp);
        for (const candidate of peer.pending.splice(0))
          await peer.pc.addIceCandidate(candidate);
      } else if (signal.type === "ice") {
        if (peer.pc.remoteDescription)
          await peer.pc.addIceCandidate(signal.candidate);
        else if (peer.pending.length < 128) peer.pending.push(signal.candidate);
      }
    } catch {
      if (current.current?.sessionId === active.sessionId) {
        closePeer(participant.userId);
        toast.error(
          "Could not connect to a room participant. Leave and rejoin to retry.",
        );
      }
    }
  };
  const join = async (community, channel) => {
    if (current.current) return toast("Leave your current voice room first.");
    const intent = crypto.randomUUID();
    update({
      intent,
      communityId: community._id,
      channelId: channel._id,
      name: channel.name,
      communityName: community.name,
      participants: [],
      sessionId: null,
      status: "joining",
    });
    try {
      const { data } = await api.get("/voice/config");
      if (current.current?.intent !== intent) return;
      const relayOnly =
        data.relayOnly || useVoiceSettingsStore.getState().relayOnly;
      if (relayOnly && !data.turnConfigured)
        throw new Error("Relay-only rooms need a configured TURN server.");
      if (!navigator.mediaDevices?.getUserMedia)
        throw new Error(
          "Voice rooms need microphone access over HTTPS or localhost.",
        );
      const stream = await openMicrophone();
      if (current.current?.intent !== intent) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }
      media.current = stream;
      // Join straight away with the microphone as it is; the noise filter can take
      // seconds to download and start, so its audio replaces the track once ready.
      denoisedStream(stream).then((clean) => {
        if (current.current?.intent !== intent || media.current !== stream) return clean.close();
        cleaned.current = clean;
        const track = clean.stream.getAudioTracks()[0];
        if (!track || clean.stream === stream) return;
        for (const peer of peers.current.values())
          peer.pc.getSenders().find((sender) => sender.track?.kind === "audio")?.replaceTrack(track).catch(() => {});
      }).catch(() => {}); // Keep the plain microphone if the filter cannot start.
      stream.getAudioTracks().forEach((track) => {
        track.enabled = !mute.current && !deaf.current;
        track.onended = () => {
          if (current.current?.intent !== intent || media.current !== stream) return;
          toast.error("Microphone disconnected. Choose another device and rejoin the room.");
          leave();
        };
      });
      config.current = {
        iceServers: data.iceServers,
        iceTransportPolicy: relayOnly ? "relay" : "all",
      };
      stopConfigRefresh.current = refreshVoiceConfiguration(
        data,
        async () => (await api.get("/voice/config")).data,
        (fresh) => {
          if (current.current?.intent !== intent) return;
          if ((relayOnly || fresh.relayOnly) && !fresh.turnConfigured)
            throw new Error("TURN unavailable");
          const next = { iceServers: fresh.iceServers,
            iceTransportPolicy: relayOnly || fresh.relayOnly ? "relay" : "all" };
          for (const peer of peers.current.values())
            peer.pc.setConfiguration({ ...peer.pc.getConfiguration(), ...next });
          config.current = next;
        },
      );
      const result = await request("voice:room-join", {
        communityId: community._id,
        channelId: channel._id,
      });
      if (current.current?.intent !== intent) {
        if (result?.sessionId) request("voice:room-leave", { sessionId: result.sessionId }).catch(() => {});
        return;
      }
      update({
        ...current.current,
        ...result,
        name: channel.name,
        communityName: community.name,
        relayOnly,
        status: "joined",
        startedAt: Date.now(),
      });
      if (mute.current || deaf.current)
        request("voice:room-status", {
          sessionId: result.sessionId,
          muted: mute.current,
          deafened: deaf.current,
        }).catch(() => {});
      for (const event of early.current.splice(0)) receive(event);
      for (const participant of result.participants.filter(
        (person) => person.sessionId !== result.sessionId,
      )) {
        if (current.current?.intent !== intent) return;
        const peer = makePeer(participant);
        await peer.pc.setLocalDescription(await peer.pc.createOffer());
        if (peers.current.get(peer.userId) !== peer) continue;
        await send(peer, {
          type: "offer",
          sdp: peer.pc.localDescription.toJSON(),
        });
        flush(peer);
      }
    } catch (error) {
      if (current.current?.intent === intent) {
        toast.error(
          error.name === "NotAllowedError"
            ? "Allow microphone access to join a voice room."
            : error.message,
        );
        leave();
      }
    }
  };
  const kick = async (communityId, channelId, person) => {
    try {
      await request("voice:room-kick", { communityId, channelId, to: person.userId, toSession: person.sessionId });
      toast("Participant disconnected. They can rejoin unless the room is locked.");
    } catch (error) { toast.error(error.message); }
  };
  const observe = async (communityId) => {
    try {
      const result = await request("voice:rooms", { communityId });
      setRooms((value) => ({
        ...value,
        ...Object.fromEntries(
          result.rooms.map((item) => [
            `${communityId}:${item.channelId}`,
            item,
          ]),
        ),
      }));
    } catch (error) {
      toast.error(error.message);
    }
  };
  useEffect(() => {
    if (!socket) return;
    const snapshot = (data) => {
      setRooms((value) => ({
        ...value,
        [`${data.communityId}:${data.channelId}`]: data,
      }));
      const active = current.current;
      if (
        !active ||
        active.communityId !== data.communityId ||
        active.channelId !== data.channelId
      )
        return;
      update({ ...active, participants: data.participants });
      for (const [id, peer] of peers.current)
        if (
          !data.participants.some(
            (person) =>
              person.userId === id && person.sessionId === peer.sessionId,
          )
        )
          closePeer(id);
    };
    const left = (data) => {
      const active = current.current;
      if (
        active &&
        active.communityId === data.communityId &&
        active.channelId === data.channelId &&
        (!active.sessionId || active.sessionId === data.sessionId)
      ) {
        cleanup();
        if (data.reason === "removed")
          toast("You no longer have access to that voice room.");
        else if (data.reason === "channelDeleted")
          toast("This voice room was deleted.");
        else if (data.reason === "kicked")
          toast("A moderator disconnected you from the voice room.");
        else if (data.reason === "movedElsewhere")
          toast("You joined this voice room on another device.");
      }
    };
    const removed = ({ communityId }) => {
      setRooms((value) =>
        Object.fromEntries(
          Object.entries(value).filter(
            ([key]) => !key.startsWith(`${communityId}:`),
          ),
        ),
      );
      if (current.current?.communityId === communityId) leave();
    };
    const disconnect = () => {
      cleanup();
      setRooms({});
    };
    socket.on("voice:room-state", snapshot);
    socket.on("voice:room-signal", receive);
    socket.on("voice:room-left", left);
    socket.on("communityRemoved", removed);
    socket.on("disconnect", disconnect);
    return () => {
      socket.off("voice:room-state", snapshot);
      socket.off("voice:room-signal", receive);
      socket.off("voice:room-left", left);
      socket.off("communityRemoved", removed);
      socket.off("disconnect", disconnect);
      leave();
    };
  }, [socket]);
  const status = () => {
    media.current?.getAudioTracks().forEach((track) => {
      track.enabled = !mute.current && !deaf.current;
    });
    for (const peer of peers.current.values())
      if (peer.audio) peer.audio.muted = deaf.current;
    if (current.current?.sessionId)
      request("voice:room-status", {
        sessionId: current.current.sessionId,
        muted: mute.current,
        deafened: deaf.current,
      }).catch(() => {});
  };
  const toggleMute = () => {
    mute.current = !mute.current;
    setMuted(mute.current);
    status();
  };
  const toggleDeafen = () => {
    deaf.current = !deaf.current;
    setDeafened(deaf.current);
    status();
  };
  const resumeAudio = () =>
    Promise.all([...peers.current.values()].map(play))
      .then(() => setPlaybackBlocked(false))
      .catch(() =>
        toast.error(
          "Check your selected speakers and browser audio permission.",
        ),
      );
  useEffect(
    () =>
      useVoiceSettingsStore.subscribe((next, previous) => {
        if (
          next.outputDeviceId === previous.outputDeviceId &&
          next.outputVolume === previous.outputVolume
        )
          return;
        for (const peer of peers.current.values()) {
          if (next.outputDeviceId !== previous.outputDeviceId)
            peer.audio?.pause();
          play(peer).catch(() => setPlaybackBlocked(true));
        }
      }),
    [],
  );
  const micStream = useCallback(() => media.current, []);
  const screen = useScreenShare({
    send: (to, payload) => {
      const participant = current.current?.participants?.find((person) => person.userId === to);
      return participant ? send(participant, payload) : Promise.resolve();
    },
    targets: room?.status === "joined" ? (room.participants || []).filter((person) => person.sessionId !== room.sessionId).map((person) => person.userId) : [],
  });
  screenRef.current = screen;
  return {
    room,
    rooms,
    connections,
    join,
    leave,
    observe,
    kick,
    muted,
    deafened,
    toggleMute,
    toggleDeafen,
    playbackBlocked,
    resumeAudio,
    micStream,
    screen,
  };
}
