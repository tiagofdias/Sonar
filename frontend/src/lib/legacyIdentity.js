import { existingIdentity, storeIdentity } from "./crypto.js";
import {
  RECOVERY_BACKUP_VERSION,
  createRecoverableIdentity,
  importIdentity,
  newRecovery,
  recoveryProof,
  unsealIdentity,
  unwrapPrivateKey,
  unwrapWithRecovery,
} from "./keyBackup.js";

// The account key exists, but only on another device that never uploaded an
// encrypted backup. The user can turn on multi-device access from that device,
// or reset the key here (older messages become unreadable on this device).
export class LegacyIdentityMissing extends Error {
  constructor(user) {
    super("Your encryption key is only on the device where you first used Sonar.");
    this.name = "LegacyIdentityMissing";
    this.user = user;
  }
}

// A backup exists; this device needs the recovery key (current backups) or,
// for backups made before recovery keys, the account password.
export class IdentityLocked extends Error {
  constructor(user, needs = "recovery") {
    super(needs === "recovery" ? "Enter your recovery key to unlock your messages on this device." : "Enter your password to unlock your encrypted messages on this device.");
    this.name = "IdentityLocked";
    this.user = user;
    this.needs = needs;
  }
}

function validUser(user, api) {
  if (!user?._id || !Object.hasOwn(user, "encryptionKey") || (user.encryptionKey !== null && typeof user.encryptionKey !== "string") || !api || typeof api.put !== "function")
    throw new Error("Account identity is unavailable.");
}

async function confirm(api, user, body) {
  const response = await api.put("/auth/encryption-key", body);
  if (response.data?._id !== user._id || response.data?.encryptionKey !== body.publicKey) throw new Error("The account encryption key could not be confirmed.");
  return response.data;
}

async function fetchBackup(api) {
  if (typeof api.get !== "function") return null;
  return (await api.get("/auth/key-backup")).data?.backup || null;
}

// Replaces a password-protected (or missing) backup with a recovery-key backup.
async function protectWithRecovery(api, user, pkcs8, password) {
  const recovery = await newRecovery(pkcs8, user._id, user.encryptionKey);
  await api.put("/auth/recovery", { currentPassword: password, backup: recovery.backup, recoveryVerifier: recovery.verifier });
  return recovery.recoveryKey;
}

// Makes sure this device holds the account's private key.
//   - Key already on this device: confirm it with the server.
//   - Recovery backup: unlock with the recovery key (IdentityLocked without it).
//   - Legacy password backup: unlock with the password, then move to a recovery key.
//   - No key yet: create one protected by a new recovery key.
//   - Key only on another device without a backup: LegacyIdentityMissing.
// `onRecoveryKey` receives a newly created recovery key, which the user must save.
export async function pairLegacyIdentity(user, api, password, { recoveryKey, onRecoveryKey = () => {} } = {}) {
  validUser(user, api);
  const local = await existingIdentity(user._id);
  if (!user.encryptionKey) {
    const created = await createRecoverableIdentity(user._id);
    const confirmed = await confirm(api, user, { publicKey: created.publicKey, backup: created.backup, recoveryVerifier: created.verifier });
    await storeIdentity(user._id, created.pair);
    onRecoveryKey(created.recoveryKey);
    return confirmed;
  }
  const backup = local?.publicKeyText === user.encryptionKey ? await fetchBackup(api).catch(() => null) : await fetchBackup(api);
  const current = backup?.publicKey === user.encryptionKey ? backup : null;
  if (local?.publicKeyText === user.encryptionKey) {
    const confirmed = await confirm(api, user, { publicKey: user.encryptionKey });
    // Move a legacy password backup to a recovery key at the next password sign-in.
    if (password && current && current.version !== RECOVERY_BACKUP_VERSION) {
      const pkcs8 = await unsealIdentity(local).catch(() => null) || await unwrapPrivateKey(current, password, user._id).catch(() => null);
      if (pkcs8) try { onRecoveryKey(await protectWithRecovery(api, user, pkcs8, password)); } finally { pkcs8.fill(0); }
    }
    return confirmed;
  }
  if (!current) throw new LegacyIdentityMissing(user);
  let pkcs8;
  if (current.version === RECOVERY_BACKUP_VERSION) {
    if (!recoveryKey) throw new IdentityLocked(user, "recovery");
    pkcs8 = await unwrapWithRecovery(current, recoveryKey, user._id);
  } else {
    if (!password) throw new IdentityLocked(user, "password");
    pkcs8 = await unwrapPrivateKey(current, password, user._id);
  }
  try {
    await storeIdentity(user._id, await importIdentity(pkcs8, user.encryptionKey));
    if (current.version !== RECOVERY_BACKUP_VERSION) onRecoveryKey(await protectWithRecovery(api, user, pkcs8, password));
  } finally {
    pkcs8.fill(0);
  }
  return confirm(api, user, { publicKey: user.encryptionKey });
}

// Replaces the account key with a new key protected by a new recovery key.
// The previous key, if this device has it, is retired locally so its
// messages stay readable here. Returns { user, recoveryKey }.
export async function rotateIdentity(user, api, password) {
  validUser(user, api);
  const created = await createRecoverableIdentity(user._id);
  const response = await api.post("/auth/encryption-key/rotate", {
    currentPassword: password,
    previousKey: user.encryptionKey,
    publicKey: created.publicKey,
    backup: created.backup,
    recoveryVerifier: created.verifier,
  });
  if (response.data?._id !== user._id || response.data?.encryptionKey !== created.publicKey) throw new Error("The new encryption key could not be confirmed.");
  await storeIdentity(user._id, created.pair);
  return { user: response.data, recoveryKey: created.recoveryKey };
}

// Issues a new recovery key for the key held on this device (the old one stops working).
export async function replaceRecoveryKey(user, api, password) {
  validUser(user, api);
  const local = await existingIdentity(user._id);
  const pkcs8 = local?.publicKeyText === user.encryptionKey ? await unsealIdentity(local).catch(() => null) : null;
  if (!pkcs8) throw new Error("This browser cannot create a recovery key for your current key. Use “Turn on multi-device access” instead.");
  try { return await protectWithRecovery(api, user, pkcs8, password); } finally { pkcs8.fill(0); }
}

// Forgotten password: reset it with the recovery key, unlock the message key,
// then issue a fresh recovery key (the old one was shown to the server as a
// reset proof). Returns { user, recoveryKey }.
export async function recoverAccount(api, email, recoveryKey, newPassword) {
  const proof = await recoveryProof(recoveryKey);
  const { data: user } = await api.post("/auth/recover", { email: email.trim(), proof, newPassword });
  let fresh = null;
  const paired = await pairLegacyIdentity(user, api, newPassword, { recoveryKey, onRecoveryKey: (key) => { fresh = key; } });
  if (!fresh) fresh = await replaceRecoveryKey(paired, api, newPassword).catch(() => null);
  return { user: paired, recoveryKey: fresh };
}

// "recovery", "password" (legacy backup), or null (no backup).
export async function backupKind(user, api) {
  const backup = await fetchBackup(api);
  if (!backup || backup.publicKey !== user?.encryptionKey) return null;
  return backup.version === RECOVERY_BACKUP_VERSION ? "recovery" : "password";
}
export async function hasKeyBackup(user, api) {
  return Boolean(await backupKind(user, api));
}
