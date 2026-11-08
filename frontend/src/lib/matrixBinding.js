// Sign only a narrowly scoped server challenge, never an arbitrary string.
// A valid signature proves possession; it does not approve the device.
const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const SESSION = /^[a-f0-9]{64}$/;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;

export async function signMatrixBinding(machine, userId, deviceId, challenge) {
  if (!machine || typeof userId !== "string" || !USER.test(userId) || typeof deviceId !== "string" || !DEVICE.test(deviceId) || typeof challenge?.token !== "string" || !TOKEN.test(challenge.token) || typeof challenge.message !== "string" || typeof challenge.expiresAt !== "string") throw new Error("Invalid device binding challenge.");
  const parts = challenge.message.split("\n");
  const expires = Date.parse(challenge.expiresAt), now = Date.now();
  if (parts.length !== 5 || parts[0] !== "sonar-matrix-device-binding-v1" || parts[1] !== userId || parts[2] !== deviceId || !SESSION.test(parts[3]) || parts[4] !== challenge.token || !Number.isFinite(expires) || expires <= now || expires > now + 180000) throw new Error("Invalid device binding challenge.");
  const actualUser = machine.userId, actualDevice = machine.deviceId;
  try {
    if (actualUser.toString() !== userId || actualDevice.toString() !== deviceId) throw new Error("The local device does not match this challenge.");
  } finally { actualUser.free(); actualDevice.free(); }
  const signatures = await machine.sign(challenge.message);
  try {
    const signature = JSON.parse(signatures.asJSON())?.[userId]?.[`ed25519:${deviceId}`];
    if (typeof signature !== "string" || !/^[A-Za-z0-9+/]{86}$/.test(signature)) throw new Error("Device signature unavailable.");
    return signature;
  } finally { signatures.free(); }
}
