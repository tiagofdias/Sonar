// Experimental local Matrix store. No account, message, or legacy identity is
// modified here. Callers must persist the descriptor and protect the store key;
// neither is a replacement for device approval or a Matrix recovery key.
import { initAsync, OlmMachine, UserId, DeviceId, StoreHandle } from "@matrix-org/matrix-sdk-crypto-wasm";

const USER = /^@[a-f0-9]{24}:[a-z0-9.-]{1,200}$/;
const DEVICE = /^[A-Za-z0-9]{1,32}$/;
const STORE = /^sonar-matrix-v2-[a-f0-9]{32}$/;
const ED25519 = /^[A-Za-z0-9+/]{43}$/;

function validate(userId, deviceId, key) {
  if (typeof userId !== "string" || !USER.test(userId) || typeof deviceId !== "string" || !DEVICE.test(deviceId)) throw new Error("Invalid Matrix device identity.");
  if (!(key instanceof Uint8Array) || key.length !== 32) throw new Error("The Matrix store key must be 32 bytes.");
  if (!globalThis.indexedDB || !globalThis.navigator?.locks || !globalThis.crypto?.getRandomValues) throw new Error("This browser cannot safely open the Matrix store.");
}

async function accessStore(userId, deviceId, storeName, key, callback) {
  return navigator.locks.request(storeName, { mode: "exclusive" }, async () => {
    await initAsync();
    const localKey = new Uint8Array(key);
    let store;
    try {
      store = await StoreHandle.openWithKey(storeName, localKey);
    } finally {
      localKey.fill(0);
    }
    let machine;
    const user = new UserId(userId), device = new DeviceId(deviceId);
    try {
      machine = await OlmMachine.initFromStore(user, device, store);
      return await callback(machine);
    } finally {
      machine?.close();
      user.free();
      device.free();
      store.free();
    }
  });
}

function publicKey(machine) {
  const keys = machine.identityKeys;
  const ed25519 = keys.ed25519;
  try { return ed25519.toBase64(); }
  finally { ed25519.free(); keys.free(); }
}

// Explicitly creates a new, randomly named device store; it never opens or
// replaces an existing Sonar or legacy device by a predictable name.
export async function createMatrixDeviceStore(userId, deviceId, key) {
  validate(userId, deviceId, key);
  const random = crypto.getRandomValues(new Uint8Array(16));
  const storeName = `sonar-matrix-v2-${Array.from(random, byte => byte.toString(16).padStart(2, "0")).join("")}`;
  return accessStore(userId, deviceId, storeName, key, machine => ({ userId, deviceId, storeName, ed25519: publicKey(machine) }));
}

// If IndexedDB was cleared or the device changed, the SDK can initialise a
// fresh account. Compare against the previously saved key before allowing any
// operation so a replacement identity is never silently used.
export async function withMatrixDeviceStore(descriptor, key, callback) {
  if (!descriptor || typeof descriptor !== "object") throw new Error("Missing Matrix device descriptor.");
  const { userId, deviceId, storeName, ed25519 } = descriptor;
  validate(userId, deviceId, key);
  if (typeof storeName !== "string" || !STORE.test(storeName) || typeof ed25519 !== "string" || !ED25519.test(ed25519) || typeof callback !== "function") throw new Error("Invalid Matrix device descriptor.");
  return accessStore(userId, deviceId, storeName, key, machine => {
    if (publicKey(machine) !== ed25519) throw new Error("Matrix device identity changed or local data was lost.");
    return callback(machine);
  });
}
