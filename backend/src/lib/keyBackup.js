import { ECDH, createHash, timingSafeEqual } from "node:crypto";

// Client-encrypted identity key backups. The server stores only ciphertext and
// can never unwrap it:
//   v2 (current): wrapped with a random recovery key the user saves. The server
//      also keeps SHA-256 of a proof derived from that key, which lets the user
//      reset a forgotten password without revealing the key.
//   v1 (legacy): wrapped with a key derived from the account password.
// The server cannot check that a backup decrypts; it only enforces shape and
// binds it to the account's published public key.
export const KEY_BACKUP_VERSION = 1;
export const RECOVERY_BACKUP_VERSION = 2;
export const KEY_BACKUP_ITERATIONS = 600000;

const base64 = (value, min, max) => {
  if (typeof value !== "string" || value.length > Math.ceil(max / 3) * 4 || !/^[A-Za-z0-9+/]+={0,2}$/.test(value)) return null;
  const bytes = Buffer.from(value, "base64");
  if (bytes.length < min || bytes.length > max || bytes.toString("base64") !== value) return null;
  return bytes;
};

export function validPublicKey(publicKey) {
  if (typeof publicKey !== "string" || !/^[A-Za-z0-9+/]{87}=$/.test(publicKey)) return false;
  const raw = Buffer.from(publicKey, "base64");
  if (raw.length !== 65 || raw[0] !== 4 || raw.toString("base64") !== publicKey) return false;
  try {
    return ECDH.convertKey(raw, "prime256v1", undefined, undefined, "uncompressed").equals(raw);
  } catch {
    return false; // Off-curve points must never bind an account.
  }
}

// Returns a normalized backup document or null when the input is malformed.
export function parseKeyBackup(input, publicKey) {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null;
  if (input.publicKey !== publicKey || !validPublicKey(publicKey)) return null;
  const { version, salt, iv, ciphertext, iterations } = input;
  // PKCS#8 for P-256 is 67-150 bytes depending on optional fields, plus a 16-byte tag.
  if (!base64(iv, 12, 12) || !base64(ciphertext, 64, 512)) return null;
  if (version === RECOVERY_BACKUP_VERSION) return { version, publicKey, iv, ciphertext };
  if (version !== KEY_BACKUP_VERSION || iterations !== KEY_BACKUP_ITERATIONS || !base64(salt, 16, 16)) return null;
  return { version, publicKey, salt, iv, ciphertext, iterations };
}

export const validVerifier = (value) => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);

// Checks a base64url reset proof against the stored verifier in constant time.
export function proofMatches(proof, verifier) {
  if (typeof proof !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(proof) || !validVerifier(verifier)) return false;
  const digest = createHash("sha256").update(Buffer.from(proof, "base64url")).digest();
  return timingSafeEqual(digest, Buffer.from(verifier, "hex"));
}

export const publicBackup = (backup) => {
  if (!backup?.ciphertext) return null;
  const { version, publicKey, salt, iv, ciphertext, iterations } = backup;
  return version === RECOVERY_BACKUP_VERSION ? { version, publicKey, iv, ciphertext } : { version, publicKey, salt, iv, ciphertext, iterations };
};
