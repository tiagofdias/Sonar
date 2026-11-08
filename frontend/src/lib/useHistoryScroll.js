import { useLayoutEffect, useRef } from "react";
export function useHistoryScroll(messages, key) {
  const scroll = useRef(null),
    nearBottom = useRef(true),
    loading = useRef(false),
    epoch = useRef(0),
    previous = useRef({ key, last: null });
  useLayoutEffect(() => {
    const node = scroll.current;
    if (!node) return;
    const last = messages.at(-1)?._id;
    const changed = previous.current.key !== key;
    if (changed) {
      nearBottom.current = true;
      loading.current = false;
      epoch.current++;
    }
    if (
      !loading.current &&
      (changed || (last !== previous.current.last && nearBottom.current))
    )
      node.scrollTop = node.scrollHeight;
    previous.current = { key, last };
  }, [messages, key]);
  const onScroll = () => {
    const node = scroll.current;
    if (node)
      nearBottom.current =
        node.scrollHeight - node.scrollTop - node.clientHeight < 100;
  };
  const loadOlder = async (action) => {
    if (loading.current || !scroll.current) return;
    const node = scroll.current,
      height = node.scrollHeight,
      top = node.scrollTop,
      currentKey = previous.current.key,
      token = epoch.current;
    loading.current = true;
    try {
      await action();
    } finally {
      requestAnimationFrame(() => {
        if (token !== epoch.current) return;
        if (scroll.current === node && previous.current.key === currentKey) {
          node.scrollTop = top + node.scrollHeight - height;
          onScroll();
        }
        loading.current = false;
      });
    }
  };
  return { scroll, onScroll, loadOlder };
}
