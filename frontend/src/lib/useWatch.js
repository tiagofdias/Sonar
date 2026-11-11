import { useCallback, useEffect, useRef, useState } from "react";
import toast from "react-hot-toast";
import { clockOffset } from "./watch";

// Watch Together state shared over the live connection. `inCall` is true while this
// person is in an answered call or a joined voice room, the only places to watch.
export function useWatch(socket, inCall) {
  const [state, setState] = useState(null);
  const [available, setAvailable] = useState(null);
  const [joined, setJoined] = useState(false);
  const [reactions, setReactions] = useState([]);
  const offset = useRef(0);
  const joinedRef = useRef(false);
  joinedRef.current = joined;

  const take = useCallback((next) => {
    if (!next) return;
    offset.current = clockOffset(next);
    setState(next);
  }, []);
  const request = useCallback((event, payload = {}) => new Promise((resolve) => {
    if (!socket?.connected) { toast.error("Reconnecting. Try again in a moment."); return resolve(null); }
    socket.timeout(8000).emit(event, payload, (error, result) => {
      if (error || result?.error) {
        toast.error(result?.error || "Watch together did not respond. Try again.");
        return resolve(null);
      }
      if (result?.state) take(result.state);
      resolve(result);
    });
  }), [socket, take]);

  const reset = useCallback(() => { setJoined(false); setState(null); setReactions([]); }, []);
  useEffect(() => {
    if (!socket) return undefined;
    const onState = (next) => { if (joinedRef.current) take(next); };
    const onAvailable = (note) => setAvailable(note?.active ? note : null);
    const onEnded = () => { reset(); setAvailable(null); };
    const onReact = (reaction) => {
      if (!joinedRef.current) return;
      const item = { ...reaction, left: 10 + Math.random() * 75 };
      setReactions((list) => [...list.slice(-20), item]);
      setTimeout(() => setReactions((list) => list.filter((entry) => entry.id !== item.id)), 3200);
    };
    const onSkipped = () => { if (joinedRef.current) toast("Skipped a sponsor segment", { id: "watch-skip", duration: 2000 }); };
    socket.on("watch:state", onState);
    socket.on("watch:available", onAvailable);
    socket.on("watch:ended", onEnded);
    socket.on("watch:react", onReact);
    socket.on("watch:skipped", onSkipped);
    return () => {
      socket.off("watch:state", onState);
      socket.off("watch:available", onAvailable);
      socket.off("watch:ended", onEnded);
      socket.off("watch:react", onReact);
      socket.off("watch:skipped", onSkipped);
    };
  }, [socket, take, reset]);
  // Leaving the call ends watching here; the server drops the viewer too.
  useEffect(() => { if (!inCall) { reset(); setAvailable(null); } }, [inCall, reset]);

  const join = useCallback(async () => { if (await request("watch:join")) setJoined(true); }, [request]);
  const leave = useCallback(() => { if (socket?.connected) socket.emit("watch:leave", {}, () => {}); reset(); }, [socket, reset]);
  return {
    state, available, joined, reactions, offset,
    join, leave,
    add: (url) => request("watch:add", { url }),
    remove: (id) => request("watch:remove", { id }),
    move: (id, to) => request("watch:move", { id, to }),
    skip: () => request("watch:skip"),
    playItem: (id) => request("watch:play-item", { id }),
    play: () => request("watch:play"),
    pause: () => request("watch:pause"),
    seek: (position) => request("watch:seek", { position }),
    react: (emoji) => request("watch:react", { emoji }),
    continueWithout: () => request("watch:continue"),
    // Quiet reports from the player; failures are retried by the next report.
    status: (value) => socket?.connected && socket.emit("watch:status", { state: value }, () => {}),
    ended: (id) => socket?.connected && socket.emit("watch:ended", { id }, () => {}),
  };
}
