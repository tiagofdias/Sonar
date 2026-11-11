export const directCallUsers = new Map();
export const voiceRoomUsers = new Map();
// Answered direct calls: call id -> [caller, callee]. Watch Together uses it to know who shares a call.
export const answeredCalls = new Map();
// Set by socket.js; called whenever someone joins or leaves a call or voice room.
export const activity = { changed: () => {} };
