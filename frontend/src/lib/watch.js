// Timing helpers for Watch Together. The server keeps the shared position; each
// player works out where it should be and corrects itself when it drifts.
export const REACTIONS = ["😂", "😮", "❤️", "👏", "🔥", "😢", "👀", "🎉"];
export const DRIFT_SECONDS = 1;
export const STALL_MS = 1500;
export const CONTINUE_AFTER_MS = 30000;

/** Server time minus local time, from a snapshot that just arrived. */
export const clockOffset = (state, receivedAt = Date.now()) => (state?.serverNow ? state.serverNow - receivedAt : 0);

/** Where the shared video should be right now, in seconds. */
export function expectedPosition(state, offset = 0, at = Date.now()) {
  if (!state || state.current < 0) return 0;
  if (!state.playing || state.hold) return state.position;
  return Math.max(0, state.position + (at + offset - state.serverNow) / 1000);
}

/** Whether this viewer's player should be playing: during a hold only the people being waited for keep playing. */
export function shouldPlay(state, me) {
  if (!state || state.current < 0 || !state.playing) return false;
  if (!state.hold) return true;
  return state.hold.waitingFor.includes(me);
}

export const drifted = (actual, expected, tolerance = DRIFT_SECONDS) => Math.abs(actual - expected) > tolerance;

export function formatTime(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  const hours = Math.floor(total / 3600), minutes = Math.floor((total % 3600) / 60), secs = total % 60;
  return hours ? `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}` : `${minutes}:${String(secs).padStart(2, "0")}`;
}

/** Names of the people everyone is waiting for, for the "Waiting for…" line. */
export function waitingNames(state) {
  const names = (state?.hold?.waitingFor || []).map((id) => state.viewers.find((viewer) => viewer.userId === id)?.name || "someone");
  if (names.length <= 2) return names.join(" and ");
  return `${names.slice(0, 2).join(", ")} and ${names.length - 2} more`;
}
