import { decodeMessage } from "./messageContent.js";
const encode = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const decode = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
let dbPromise;
function database() {
  return (dbPromise ||= new Promise((resolve, reject) => {
    const request = indexedDB.open("sonar-keys", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("keys");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  }));
}
async function read(id) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const r = db.transaction("keys").objectStore("keys").get(id);
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}
// A read and insert share one read-write transaction, including across tabs.
async function rememberFirst(id, value, mismatchMessage) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("keys", "readwrite");
    const store = tx.objectStore("keys");
    const request = store.get(id);
    let selected;
    let mismatch = false;
    request.onsuccess = () => {
      selected = request.result;
      if (selected === undefined) {
        selected = value;
        store.add(value, id);
      } else if (mismatchMessage && selected !== value) {
        mismatch = true;
        tx.abort();
      }
    };
    tx.oncomplete = () => resolve(selected);
    tx.onabort = () => reject(mismatch ? new Error(mismatchMessage) : tx.error || new Error("Key storage transaction aborted."));
    tx.onerror = () => {}; // The abort handler reports transaction failures.
  });
}
const identities = new Map();
export function identity(id) {
  if (!identities.has(id))
    identities.set(
      id,
      loadIdentity(id).catch((error) => {
        identities.delete(id);
        throw error;
      }),
    );
  return identities.get(id);
}
async function loadIdentity(id) {
  let pair = await read(id);
  if (!pair) {
    pair = await crypto.subtle.generateKey(
      { name: "ECDH", namedCurve: "P-256" },
      false,
      ["deriveKey"],
    );
    pair = await rememberFirst(id, pair);
  }
  return {
    ...pair,
    publicKeyText: encode(await crypto.subtle.exportKey("raw", pair.publicKey)),
  };
}

// Installs an identity unlocked from the encrypted backup or newly rotated.
// A different key already on this device is retired, not discarded, so
// messages encrypted for it remain readable here.
export async function storeIdentity(id, pair) {
  const previous = await existingIdentity(id);
  const next = encode(await crypto.subtle.exportKey("raw", pair.publicKey));
  const retire = previous && previous.publicKeyText !== next ? { privateKey: previous.privateKey, publicKey: previous.publicKey } : null;
  const db = await database();
  await new Promise((resolve, reject) => {
    const tx = db.transaction("keys", "readwrite");
    const store = tx.objectStore("keys");
    const retired = store.get(`retired:${id}`);
    retired.onsuccess = () => {
      if (retire) store.put([retire, ...(retired.result || [])].slice(0, 8), `retired:${id}`);
      store.put(pair, id);
    };
    tx.oncomplete = resolve;
    tx.onabort = () => reject(tx.error || new Error("Could not save the encryption key on this device."));
    tx.onerror = () => {};
  });
  identities.delete(id);
}
async function retiredIdentities(id) {
  return (await read(`retired:${id}`)) || [];
}

// Read an existing legacy identity without creating one on a new device.
export async function existingIdentity(id) {
  const pair = await read(id);
  if (!pair?.privateKey || !pair?.publicKey) return null;
  return { ...pair, publicKeyText: encode(await crypto.subtle.exportKey("raw", pair.publicKey)) };
}

// Unlike identity(), this lookup must never create a replacement legacy key.
export async function proveExistingLegacyIdentity(accountId, userId, deviceId, challenge) {
  if (!/^[a-f0-9]{24}$/.test(accountId) || !userId.startsWith(`@${accountId}:`) || !/^[A-Za-z0-9]{1,32}$/.test(deviceId)) throw new Error("Invalid identity.");
  const pair = await read(accountId);
  if (!pair?.privateKey || !pair?.publicKey) throw new Error("The original browser encryption key is unavailable.");
  const localPublicKey = encode(await crypto.subtle.exportKey("raw", pair.publicKey));
  const { token, message, legacyPublicKey, serverPublicKey, iv, ciphertext, expiresAt } = challenge || {};
  const parts = typeof message === "string" ? message.split("\n") : [];
  const expiry = Date.parse(expiresAt);
  if (legacyPublicKey !== localPublicKey || typeof token !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(token) || parts.length !== 5 || parts[0] !== "sonar-legacy-bootstrap-v1" || parts[1] !== userId || parts[2] !== deviceId || !/^[a-f0-9]{64}$/.test(parts[3]) || parts[4] !== token || !Number.isFinite(expiry) || expiry <= Date.now() || expiry > Date.now() + 180000) throw new Error("Invalid legacy key challenge.");
  let serverBytes, ivBytes, encrypted;
  try { serverBytes = decode(serverPublicKey); ivBytes = decode(iv); encrypted = decode(ciphertext); } catch { throw new Error("Invalid legacy key challenge."); }
  if (serverBytes.length !== 65 || serverBytes[0] !== 4 || ivBytes.length !== 12 || encrypted.length !== 48) throw new Error("Invalid legacy key challenge.");
  const publicKey = await crypto.subtle.importKey("raw", serverBytes, { name: "ECDH", namedCurve: "P-256" }, false, []);
  const key = await crypto.subtle.deriveKey({ name: "ECDH", public: publicKey }, pair.privateKey, { name: "AES-GCM", length: 256 }, false, ["decrypt"]);
  const proof = new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: ivBytes, additionalData: new TextEncoder().encode(message), tagLength: 128 }, key, encrypted));
  if (proof.length !== 32) throw new Error("Invalid legacy key proof.");
  try { return encode(proof).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", ""); }
  finally { proof.fill(0); }
}
async function keyFor(me, peer) {
  if (!peer.encryptionKey)
    throw new Error("This person has not set up encryption yet.");
  const publicKey = await crypto.subtle.importKey(
    "raw",
    decode(peer.encryptionKey),
    { name: "ECDH", namedCurve: "P-256" },
    false,
    [],
  );
  await rememberFirst(
    `trust:${me}:${peer._id}`,
    peer.encryptionKey,
    "Contact key changed. Sending is blocked. Verify their identity first.",
  );
  const own = await identity(me);
  return crypto.subtle.deriveKey(
    { name: "ECDH", public: publicKey },
    own.privateKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

// Contact-key changes require explicit, out-of-band verification. This does
// not make the server trustworthy or recover messages made for previous keys.
export async function contactKeyStatus(me, peer) {
  const saved = await read(`trust:${me}:${peer._id}`);
  return { saved: saved || null, changed: Boolean(saved && saved !== peer.encryptionKey) };
}
// Called after the user compared the contact's safety code (the key's SHA-256
// shown in groups) with them in person or over a call and confirmed it matches.
export async function acceptVerifiedContactKey(me, peer, expectedPrevious) {
  if (!expectedPrevious || !peer?.encryptionKey || expectedPrevious === peer.encryptionKey)
    throw new Error("Contact key change is no longer pending.");
  await crypto.subtle.importKey("raw", decode(peer.encryptionKey), { name: "ECDH", namedCurve: "P-256" }, false, []);
  const db = await database();
  await new Promise((resolve, reject) => {
    const tx = db.transaction("keys", "readwrite");
    const store = tx.objectStore("keys");
    const key = `trust:${me}:${peer._id}`;
    const request = store.get(key);
    let changed = false;
    request.onsuccess = () => {
      if (request.result !== expectedPrevious) { changed = true; tx.abort(); return; }
      store.put(peer.encryptionKey, key);
    };
    tx.oncomplete = resolve;
    tx.onabort = () => reject(new Error(changed ? "Stored contact key changed. Review it again." : "Could not save the verified key."));
    tx.onerror = () => {};
  });
}
const context = (sender, receiver, scope = "") =>
  new TextEncoder().encode(
    `sonar:v1:${sender}:${receiver}${scope ? `:channel:${scope}` : ""}`,
  );
export async function encrypt(me, peer, text, scope = "") {
  const key = await keyFor(me, peer);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: context(me, peer._id, scope) },
    key,
    new TextEncoder().encode(text),
  );
  return { version: 1, iv: encode(iv), ciphertext: encode(ciphertext) };
}
export async function decrypt(me, peer, message) {
  try {
    if (!message.ciphertext)
      return {
        ...message,
        text: "Legacy message unavailable in the encrypted client.",
      };
    const params = {
      name: "AES-GCM",
      iv: decode(message.iv),
      additionalData: context(
        message.senderId,
        message.receiverId,
        message.channelId || "",
      ),
    };
    let bytes;
    try {
      bytes = await crypto.subtle.decrypt(params, await keyFor(me, peer), decode(message.ciphertext));
    } catch (error) {
      // Messages sent before this account's key was rotated need a retired key.
      const retired = await retiredIdentities(me);
      if (!retired.length || !peer.encryptionKey) throw error;
      const publicKey = await crypto.subtle.importKey("raw", decode(peer.encryptionKey), { name: "ECDH", namedCurve: "P-256" }, false, []);
      for (const old of retired) {
        try {
          const key = await crypto.subtle.deriveKey({ name: "ECDH", public: publicKey }, old.privateKey, { name: "AES-GCM", length: 256 }, false, ["decrypt"]);
          bytes = await crypto.subtle.decrypt(params, key, decode(message.ciphertext));
          break;
        } catch { /* try the next retired key */ }
      }
      if (!bytes) throw error;
    }
    return { ...message, ...decodeMessage(new TextDecoder().decode(bytes)) };
  } catch {
    return {
      ...message,
      text: "Unable to decrypt on this device.",
      unreadable: true,
    };
  }
}
export async function fingerprint(publicKey) {
  const hash = await crypto.subtle.digest("SHA-256", decode(publicKey));
  return [...new Uint8Array(hash)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
    .match(/.{1,4}/g)
    .join(" ");
}
