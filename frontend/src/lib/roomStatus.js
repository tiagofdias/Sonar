// What the voice room banner says about the audio links to everyone else in the room.
// Keep the wording the same as room_status in desktop/src/ui.rs.
export function roomStatus({ joining, others, connected, failed = 0 }) {
  if (joining) return "Connecting…";
  if (others <= 0) return "Waiting for others to join";
  if (failed > 0) return others === 1 ? "Couldn't connect · leave and rejoin" : `Couldn't reach ${failed} of ${others} · leave and rejoin`;
  if (connected >= others) return others === 1 ? "Connected" : "Connected to everyone";
  if (connected === 0) return "Connecting…";
  return `Connected to ${connected} of ${others}`;
}
