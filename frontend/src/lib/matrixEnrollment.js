// Client-side preparation of a proof of possession for the experimental
// enrollment service. This does not approve or verify a device.
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const SESSION = /^[a-f0-9]{64}$/;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;
const BASE64 = /^[A-Za-z0-9+/]+$/;

export async function signMatrixEnrollment(machine, userId, deviceId, keys, challenge) {
  if (!machine || typeof userId !== "string" || !USER.test(userId) || typeof deviceId !== "string" || !DEVICE.test(deviceId) || !keys || keys.user_id !== userId || keys.device_id !== deviceId || !Array.isArray(keys.algorithms) || keys.algorithms.length !== 2 || new Set(keys.algorithms).size !== 2 || !["m.olm.v1.curve25519-aes-sha2", "m.megolm.v1.aes-sha2"].every(x => keys.algorithms.includes(x)) || typeof challenge?.token !== "string" || !TOKEN.test(challenge.token) || typeof challenge.message !== "string" || typeof challenge.expiresAt !== "string") throw new Error("Invalid device enrollment challenge.");
  const ed = `ed25519:${deviceId}`, curve = `curve25519:${deviceId}`;
  const ownUser = machine.userId, ownDevice = machine.deviceId, identity = machine.identityKeys;
  const ownEd = identity.ed25519, ownCurve = identity.curve25519;
  try {
    if (ownUser.toString() !== userId || ownDevice.toString() !== deviceId || keys.keys?.[ed] !== ownEd.toBase64() || keys.keys?.[curve] !== ownCurve.toBase64() || typeof keys.signatures?.[userId]?.[ed] !== "string" || !BASE64.test(keys.signatures[userId][ed])) throw new Error("The local device does not match this enrollment.");
  } finally { ownUser.free(); ownDevice.free(); ownEd.free(); ownCurve.free(); identity.free(); }
  // Mirror the server's strict normalized public-key shape before hashing.
  const normalized = { user_id: userId, device_id: deviceId, algorithms: [...keys.algorithms], keys: { [curve]: keys.keys[curve], [ed]: keys.keys[ed] }, signatures: { [userId]: { [ed]: keys.signatures[userId][ed] } } };
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(normalized)));
  const hash = Array.from(new Uint8Array(bytes), byte => byte.toString(16).padStart(2, "0")).join("");
  const parts = challenge.message.split("\n"), expires = Date.parse(challenge.expiresAt), now = Date.now();
  if (parts.length !== 6 || parts[0] !== "sonar-matrix-device-enrollment-v1" || parts[1] !== userId || parts[2] !== deviceId || !SESSION.test(parts[3]) || parts[4] !== hash || parts[5] !== challenge.token || !Number.isFinite(expires) || expires <= now || expires > now + 180000) throw new Error("Invalid device enrollment challenge.");
  const signatures = await machine.sign(challenge.message);
  try {
    const signature = JSON.parse(signatures.asJSON())?.[userId]?.[ed];
    if (typeof signature !== "string" || !/^[A-Za-z0-9+/]{86}$/.test(signature)) throw new Error("Device signature unavailable.");
    return signature;
  } finally { signatures.free(); }
}
