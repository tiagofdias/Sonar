import { useCallback, useEffect, useRef, useState } from "react";
import { usePreferencesStore } from "../store/usePreferencesStore";

const SHOW_MS = 4000, SEND_GAP_MS = 2500;

// Who is typing here, and a function to call as you type. Turned off both ways by the privacy setting.
export function useTyping(socket, target) {
  const key = JSON.stringify(target || null);
  const hidden = usePreferencesStore((state) => state.typingHidden);
  const [people, setPeople] = useState({});
  const lastSent = useRef(0);
  useEffect(() => {
    setPeople({});
    if (!socket || !target || hidden) return undefined;
    const here = (event) => (target.to ? event.from === target.to && !event.communityId : event.communityId === target.communityId && event.channelId === target.channelId);
    const heard = (event) => {
      if (here(event) && !usePreferencesStore.getState().isBlocked(event.from)) setPeople((current) => ({ ...current, [event.from]: Date.now() + SHOW_MS }));
    };
    const clean = setInterval(() => setPeople((current) => {
      const left = Object.entries(current).filter(([, until]) => until > Date.now());
      return left.length === Object.keys(current).length ? current : Object.fromEntries(left);
    }), 1000);
    socket.on("typing", heard);
    return () => { socket.off("typing", heard); clearInterval(clean); };
    // The key stands in for the target object.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [socket, key, hidden]);
  const typed = useCallback(() => {
    if (!socket || !target || hidden || Date.now() - lastSent.current < SEND_GAP_MS) return;
    lastSent.current = Date.now();
    socket.emit("typing", target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [socket, key, hidden]);
  return { typing: Object.keys(people), typed };
}

export function typingText(names) {
  if (!names.length) return "";
  if (names.length === 1) return `${names[0]} is typing...`;
  if (names.length === 2) return `${names[0]} and ${names[1]} are typing...`;
  return "Several people are typing...";
}
