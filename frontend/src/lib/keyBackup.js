// Password-encrypted identity key backup (format v1), shared by the web and
// desktop clients. The private key is wrapped on the device before upload:
//   wrapping key = PBKDF2-HMAC-SHA256(password, 16-byte salt, 600000) -> AES-256
//   ciphertext   = AES-256-GCM(PKCS#8 private key), 12-byte IV, 128-bit tag
//   AAD          = "sonar-key-backup-v1:<accountId>:<base64 raw public key>"
// The server stores only the result. desktop/src/key_backup.rs implements the
// same format and both are checked against tests/key-backup-fixture.json.
export const KEY_BACKUP_VERSION = 1;
export const KEY_BACKUP_ITERATIONS = 600000;

const encode = (bytes) => {
  let text = "";
  for (const byte of new Uint8Array(bytes)) text += String.fromCharCode(byte);
  return btoa(text);
};
const decode = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const aad = (accountId, publicKey) => new TextEncoder().encode(`sonar-key-backup-v1:${accountId}:${publicKey}`);

async function wrappingKey(password, salt, iterations, usage) {
  if (typeof password !== "string" || !password) throw new Error("Enter your password.");
  const material = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations },
    material,
    { name: "AES-GCM", length: 256 },
    false,
    [usage],
  );
}

export async function wrapPrivateKey(pkcs8, password, accountId, publicKey, { salt, iv } = {}) {
  salt ||= crypto.getRandomValues(new Uint8Array(16));
  iv ||= crypto.getRandomValues(new Uint8Array(12));
  const key = await wrappingKey(password, salt, KEY_BACKUP_ITERATIONS, "encrypt");
  const ciphertext = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad(accountId, publicKey) }, key, pkcs8);
  return {
    version: KEY_BACKUP_VERSION,
    publicKey,
    salt: encode(salt),
    iv: encode(iv),
    ciphertext: encode(ciphertext),
    iterations: KEY_BACKUP_ITERATIONS,
  };
}

export class WrongKeyPassword extends Error {
  constructor() {
    super("That password could not unlock your encrypted messages.");
    this.name = "WrongKeyPassword";
  }
}

// Returns the PKCS#8 private key bytes. Callers must import them as a
// non-extractable key and drop the bytes promptly.
export async function unwrapPrivateKey(backup, password, accountId) {
  if (backup?.version !== KEY_BACKUP_VERSION || backup.iterations !== KEY_BACKUP_ITERATIONS) throw new Error("This encrypted key needs a newer version of Sonar.");
  const key = await wrappingKey(password, decode(backup.salt), backup.iterations, "decrypt");
  try {
    return new Uint8Array(await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: decode(backup.iv), additionalData: aad(accountId, backup.publicKey) },
      key,
      decode(backup.ciphertext),
    ));
  } catch {
    throw new WrongKeyPassword();
  }
}

export async function rewrapBackup(backup, currentPassword, nextPassword, accountId) {
  const pkcs8 = await unwrapPrivateKey(backup, currentPassword, accountId);
  try {
    return await wrapPrivateKey(pkcs8, nextPassword, accountId, backup.publicKey);
  } finally {
    pkcs8.fill(0);
  }
}

// Creates a fresh identity whose private key is exported once for wrapping
// and then held only as a non-extractable CryptoKey.
export async function createBackedUpIdentity(password, accountId) {
  const generated = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveKey"]);
  const publicKey = encode(await crypto.subtle.exportKey("raw", generated.publicKey));
  const pkcs8 = new Uint8Array(await crypto.subtle.exportKey("pkcs8", generated.privateKey));
  try {
    const backup = await wrapPrivateKey(pkcs8, password, accountId, publicKey);
    const pair = await importIdentity(pkcs8, publicKey);
    return { pair, publicKey, backup };
  } finally {
    pkcs8.fill(0);
  }
}

const base64url = (text) => text.replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
export async function importIdentity(pkcs8, publicKey) {
  // Confirm the private key really belongs to the published public key before
  // keeping it; a mismatched pair would silently break every conversation.
  const check = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "ECDH", namedCurve: "P-256" }, true, ["deriveKey"]);
  const jwk = await crypto.subtle.exportKey("jwk", check);
  const raw = decode(publicKey);
  const matches = raw.length === 65 && raw[0] === 4 && jwk.x === base64url(encode(raw.slice(1, 33))) && jwk.y === base64url(encode(raw.slice(33)));
  jwk.d = "";
  if (!matches) throw new Error("The encrypted key does not match this account.");
  const privateKey = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "ECDH", namedCurve: "P-256" }, false, ["deriveKey"]);
  const imported = await crypto.subtle.importKey("raw", decode(publicKey), { name: "ECDH", namedCurve: "P-256" }, true, []);
  // A sealed copy under a non-extractable device key lets this browser issue a
  // new recovery key later. Page script could use either key alike, so this
  // adds no exposure beyond the non-extractable ECDH key itself.
  const sealKey = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, sealKey, pkcs8);
  return { privateKey, publicKey: imported, sealKey, sealed: { iv, data } };
}

// ----- Recovery key backup (format v2) ---------------------------------------
// The message key is wrapped with a random recovery key the user saves, not
// with the account password, so the server (which sees the password at sign-in)
// cannot unlock it. The same recovery key resets a forgotten password.
//   recovery key = 15 random bytes, shown as 24 Crockford base32 characters
//   wrapping key = HMAC-SHA256(recoveryKey, "sonar-recovery-wrap-v2:<accountId>")
//   ciphertext   = AES-256-GCM(PKCS#8), 12-byte IV,
//                  AAD "sonar-key-backup-v2:<accountId>:<publicKey>"
//   reset proof  = HMAC-SHA256(recoveryKey, "sonar-recovery-proof-v2");
//                  the server stores only SHA-256(proof)
export const RECOVERY_BACKUP_VERSION = 2;
const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function formatRecoveryKey(bytes) {
  let bits = 0, value = 0, out = "";
  for (const byte of bytes) {
    value = (value << 8) | byte; bits += 8;
    while (bits >= 5) { out += ALPHABET[(value >>> (bits - 5)) & 31]; bits -= 5; }
  }
  return out.match(/.{1,4}/g).join("-");
}
export class WrongRecoveryKey extends Error {
  constructor() { super("That recovery key is not correct."); this.name = "WrongRecoveryKey"; }
}
export function parseRecoveryKey(text) {
  const clean = String(text || "").toUpperCase().replace(/[\s-]/g, "").replace(/O/g, "0").replace(/[IL]/g, "1");
  if (clean.length !== 24 || [...clean].some((c) => !ALPHABET.includes(c))) throw new WrongRecoveryKey();
  const out = new Uint8Array(15);
  let bits = 0, value = 0, index = 0;
  for (const char of clean) {
    value = (value << 5) | ALPHABET.indexOf(char); bits += 5;
    if (bits >= 8) { out[index++] = (value >>> (bits - 8)) & 255; bits -= 8; }
  }
  return out;
}
async function hmac(secret, label) {
  const key = await crypto.subtle.importKey("raw", secret, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(label)));
}
async function recoveryWrapKey(secret, accountId, usage) {
  const raw = await hmac(secret, `sonar-recovery-wrap-v2:${accountId}`);
  try { return await crypto.subtle.importKey("raw", raw, "AES-GCM", false, [usage]); } finally { raw.fill(0); }
}
const aad2 = (accountId, publicKey) => new TextEncoder().encode(`sonar-key-backup-v2:${accountId}:${publicKey}`);
const hex = (bytes) => [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");

/** The proof sent to reset a password (base64url). */
export async function recoveryProof(recoveryKey) {
  return base64url(encode(await hmac(parseRecoveryKey(recoveryKey), "sonar-recovery-proof-v2")));
}
/** What the server stores to check a reset proof: hex SHA-256 of the proof bytes. */
export async function recoveryVerifier(recoveryKey) {
  const proof = await hmac(parseRecoveryKey(recoveryKey), "sonar-recovery-proof-v2");
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", proof)));
}
export async function wrapWithRecovery(pkcs8, recoveryKey, accountId, publicKey, { iv } = {}) {
  iv ||= crypto.getRandomValues(new Uint8Array(12));
  const key = await recoveryWrapKey(parseRecoveryKey(recoveryKey), accountId, "encrypt");
  const ciphertext = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad2(accountId, publicKey) }, key, pkcs8);
  return { version: RECOVERY_BACKUP_VERSION, publicKey, iv: encode(iv), ciphertext: encode(ciphertext) };
}
export async function unwrapWithRecovery(backup, recoveryKey, accountId) {
  if (backup?.version !== RECOVERY_BACKUP_VERSION) throw new Error("This encrypted key needs a newer version of Sonar.");
  const key = await recoveryWrapKey(parseRecoveryKey(recoveryKey), accountId, "decrypt");
  try {
    return new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: decode(backup.iv), additionalData: aad2(accountId, backup.publicKey) }, key, decode(backup.ciphertext)));
  } catch {
    throw new WrongRecoveryKey();
  }
}
/** A new recovery key for an existing private key: { recoveryKey, backup, verifier }. */
export async function newRecovery(pkcs8, accountId, publicKey) {
  const recoveryKey = formatRecoveryKey(crypto.getRandomValues(new Uint8Array(15)));
  const backup = await wrapWithRecovery(pkcs8, recoveryKey, accountId, publicKey);
  return { recoveryKey, backup, verifier: await recoveryVerifier(recoveryKey) };
}
/** A fresh identity protected by a new recovery key. */
export async function createRecoverableIdentity(accountId) {
  const generated = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveKey"]);
  const publicKey = encode(await crypto.subtle.exportKey("raw", generated.publicKey));
  const pkcs8 = new Uint8Array(await crypto.subtle.exportKey("pkcs8", generated.privateKey));
  try {
    const recovery = await newRecovery(pkcs8, accountId, publicKey);
    return { pair: await importIdentity(pkcs8, publicKey), publicKey, ...recovery };
  } finally {
    pkcs8.fill(0);
  }
}
/** Recovers the PKCS#8 bytes kept sealed on this device (see importIdentity). */
export async function unsealIdentity(pair) {
  if (!pair?.sealKey || !pair?.sealed) return null;
  return new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: pair.sealed.iv }, pair.sealKey, pair.sealed.data));
}
