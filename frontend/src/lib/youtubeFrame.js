// Drives YouTube's official embedded player by messages, without loading YouTube's
// own script into Sonar's page (less memory, and no third-party code beside the keys).
// These are the same messages YouTube's IFrame API script sends.
export const YOUTUBE_ORIGIN = "https://www.youtube-nocookie.com";
export const PLAYER_STATE = { unstarted: -1, ended: 0, playing: 1, paused: 2, buffering: 3, cued: 5 };

export function createYouTubeFrame(container, { videoId, start = 0, onReady, onState, onTime, onDuration }) {
  const iframe = document.createElement("iframe");
  const params = new URLSearchParams({ enablejsapi: "1", origin: window.location.origin, playsinline: "1", rel: "0", start: String(Math.floor(start)) });
  iframe.src = `${YOUTUBE_ORIGIN}/embed/${encodeURIComponent(videoId)}?${params}`;
  iframe.title = "YouTube video player";
  iframe.allow = "autoplay; encrypted-media; picture-in-picture; fullscreen";
  iframe.allowFullscreen = true;
  // YouTube refuses to play embeds that send no referrer; the rest of Sonar sends none.
  iframe.referrerPolicy = "strict-origin-when-cross-origin";
  container.appendChild(iframe);

  let ready = false;
  const waiting = [];
  const deliver = (message) => iframe.contentWindow?.postMessage(message, YOUTUBE_ORIGIN);
  const command = (func, args = []) => {
    const message = JSON.stringify({ event: "command", func, args, id: 1, channel: "widget" });
    if (ready) deliver(message);
    else if (waiting.length < 20) waiting.push(message);
  };
  // The player only starts reporting once it hears "listening"; repeat until it answers.
  const hello = () => deliver(JSON.stringify({ event: "listening", id: 1, channel: "widget" }));
  let tries = 0;
  const greeter = setInterval(() => { if (ready || ++tries > 80) clearInterval(greeter); else hello(); }, 250);
  iframe.addEventListener("load", hello);

  const receive = (event) => {
    if (event.origin !== YOUTUBE_ORIGIN || event.source !== iframe.contentWindow) return;
    let data;
    try { data = typeof event.data === "string" ? JSON.parse(event.data) : event.data; } catch { return; }
    if (!data || typeof data !== "object") return;
    if (!ready && (data.event === "onReady" || data.event === "initialDelivery" || data.event === "infoDelivery")) {
      ready = true;
      clearInterval(greeter);
      for (const message of waiting.splice(0)) deliver(message);
      onReady?.();
    }
    if (data.event === "onStateChange" && typeof data.info === "number") onState?.(data.info);
    const info = data.info && typeof data.info === "object" ? data.info : null;
    if (info) {
      if (typeof info.playerState === "number") onState?.(info.playerState);
      if (typeof info.currentTime === "number" && Number.isFinite(info.currentTime)) onTime?.(info.currentTime);
      if (typeof info.duration === "number" && info.duration > 0) onDuration?.(info.duration);
    }
  };
  window.addEventListener("message", receive);

  return {
    play: () => command("playVideo"),
    pause: () => command("pauseVideo"),
    seek: (seconds) => command("seekTo", [Math.max(0, seconds), true]),
    load: (id, seconds = 0) => command("loadVideoById", [{ videoId: id, startSeconds: Math.max(0, Math.floor(seconds)) }]),
    get ready() { return ready; },
    /** Removes the player completely so its memory is given back. */
    destroy() {
      clearInterval(greeter);
      window.removeEventListener("message", receive);
      iframe.removeEventListener("load", hello);
      iframe.src = "about:blank";
      iframe.remove();
    },
  };
}
