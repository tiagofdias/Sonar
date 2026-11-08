import "fake-indexeddb/auto";
import { test } from "node:test";
import assert from "node:assert/strict";
import { identity, existingIdentity, encrypt, decrypt, fingerprint, contactKeyStatus, acceptVerifiedContactKey } from "./crypto.js";
import { pairLegacyIdentity, rotateIdentity, recoverAccount, replaceRecoveryKey, LegacyIdentityMissing, IdentityLocked } from "./legacyIdentity.js";
import { createBackedUpIdentity, formatRecoveryKey, parseRecoveryKey, recoveryVerifier, unwrapWithRecovery, WrongRecoveryKey } from "./keyBackup.js";
import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { encodeMessage, encodeReaction, REACTION_EMOJI } from "./messageContent.js";
async function forgetDeviceKey(id) {
  const db = await new Promise((resolve, reject) => { const r = indexedDB.open("sonar-keys", 1); r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error); });
  await new Promise((resolve) => { const tx = db.transaction("keys", "readwrite"); tx.objectStore("keys").delete(id); tx.oncomplete = resolve; });
}
// A minimal stand-in for the server routes the pairing code calls.
function fakeServer(user, password = "password-1234") {
  const state = { user: { ...user, email: "user@example.invalid" }, password, backup: null, verifier: null, puts: 0, sawProof: null };
  state.api = {
    async put(path, body) {
      if (path === "/auth/recovery") {
        assert.equal(body.currentPassword, state.password);
        assert.equal(body.backup.version, 2);
        state.backup = body.backup; state.verifier = body.recoveryVerifier;
        return { data: { ok: true } };
      }
      assert.equal(path, "/auth/encryption-key");
      if (state.user.encryptionKey && state.user.encryptionKey !== body.publicKey) throw Object.assign(new Error("conflict"), { response: { status: 409 } });
      if (body.backup && state.user.encryptionKey) throw Object.assign(new Error("backup only on first key"), { response: { status: 409 } });
      state.puts++;
      state.user.encryptionKey = body.publicKey;
      if (body.backup) { state.backup = body.backup; state.verifier = body.recoveryVerifier; }
      return { data: { ...state.user } };
    },
    async get(path) { assert.equal(path, "/auth/key-backup"); return { data: { backup: state.backup } }; },
    async post(path, body) {
      if (path === "/auth/recover") {
        state.sawProof = body.proof;
        const digest = createHash("sha256").update(Buffer.from(body.proof, "base64url")).digest("hex");
        if (body.email !== state.user.email || digest !== state.verifier) throw Object.assign(new Error("mismatch"), { response: { status: 400 } });
        state.password = body.newPassword;
        return { data: { ...state.user } };
      }
      assert.equal(path, "/auth/encryption-key/rotate");
      assert.equal(body.previousKey, state.user.encryptionKey);
      state.user.encryptionKey = body.publicKey; state.backup = body.backup; state.verifier = body.recoveryVerifier;
      return { data: { ...state.user } };
    },
  };
  return state;
}
test("recovery keys round-trip through their printable form", async () => {
  const bytes = crypto.getRandomValues(new Uint8Array(15));
  const text = formatRecoveryKey(bytes);
  assert.match(text, /^([0-9A-HJKMNP-TV-Z]{4}-){5}[0-9A-HJKMNP-TV-Z]{4}$/);
  assert.deepEqual(parseRecoveryKey(text.toLowerCase().replaceAll("-", " ")), bytes);
  assert.deepEqual(parseRecoveryKey(text.replaceAll("0", "O").replaceAll("1", "I")), bytes);
  assert.throws(() => parseRecoveryKey("too-short"), WrongRecoveryKey);
});
test("pairing never creates or replaces a key without a backup and its secret", async () => {
  const other = await identity("pairing-existing-server-key");
  const server = fakeServer({ _id: "pairing-unfamiliar-browser", encryptionKey: other.publicKeyText });
  await assert.rejects(pairLegacyIdentity({ _id: "pairing-missing-field" }, server.api), /Account identity is unavailable/);
  await assert.rejects(pairLegacyIdentity(server.user, server.api, "password-1234"), LegacyIdentityMissing);
  assert.equal(await existingIdentity(server.user._id), null);
  const local = await identity(server.user._id);
  await assert.rejects(pairLegacyIdentity(server.user, server.api, "password-1234"), LegacyIdentityMissing);
  assert.equal(server.puts, 0);
  assert.equal((await existingIdentity(server.user._id)).publicKeyText, local.publicKeyText);
  assert.equal((await pairLegacyIdentity({ ...server.user, encryptionKey: local.publicKeyText }, fakeServer({ ...server.user, encryptionKey: local.publicKeyText }).api)).encryptionKey, local.publicKeyText);
});
test("a new account gets a recovery key; a second device needs it, never the password", async () => {
  const server = fakeServer({ _id: "0123456789abcdef0123cafe", encryptionKey: null });
  let recoveryKey;
  const created = await pairLegacyIdentity(server.user, server.api, undefined, { onRecoveryKey: (key) => { recoveryKey = key; } });
  assert.ok(recoveryKey);
  assert.equal(server.backup.version, 2);
  assert.equal(server.backup.salt, undefined, "no password-derived wrapping");
  assert.equal(server.verifier, await recoveryVerifier(recoveryKey));
  const peer = { _id: "0123456789abcdef0123beef", encryptionKey: (await identity("0123456789abcdef0123beef")).publicKeyText };
  const sent = { ...(await encrypt(server.user._id, peer, "hello from device one")), senderId: server.user._id, receiverId: peer._id };
  await forgetDeviceKey(server.user._id);
  await assert.rejects(pairLegacyIdentity(server.user, server.api, "password-1234"), (e) => e instanceof IdentityLocked && e.needs === "recovery");
  const wrong = formatRecoveryKey(new Uint8Array(15));
  await assert.rejects(pairLegacyIdentity(server.user, server.api, undefined, { recoveryKey: wrong }), WrongRecoveryKey);
  await pairLegacyIdentity(server.user, server.api, undefined, { recoveryKey });
  assert.equal((await existingIdentity(server.user._id)).publicKeyText, created.encryptionKey);
  assert.equal((await decrypt(server.user._id, peer, sent)).text, "hello from device one");
  await assert.rejects(unwrapWithRecovery(server.backup, recoveryKey, "0123456789abcdef0123dead"), WrongRecoveryKey);
  // This device can issue a new recovery key; the old one then stops working.
  const replacement = await replaceRecoveryKey(server.user, server.api, "password-1234");
  await assert.rejects(unwrapWithRecovery(server.backup, recoveryKey, server.user._id), WrongRecoveryKey);
  assert.ok((await unwrapWithRecovery(server.backup, replacement, server.user._id)).length > 60);
});
test("a legacy password backup unlocks once and moves to a recovery key", async () => {
  const id = "0123456789abcdef0123c0de";
  const legacy = await createBackedUpIdentity("password-1234", id);
  const server = fakeServer({ _id: id, encryptionKey: legacy.publicKey });
  server.backup = legacy.backup;
  await assert.rejects(pairLegacyIdentity(server.user, server.api), (e) => e instanceof IdentityLocked && e.needs === "password");
  let recoveryKey;
  await pairLegacyIdentity(server.user, server.api, "password-1234", { onRecoveryKey: (key) => { recoveryKey = key; } });
  assert.equal(server.backup.version, 2);
  assert.ok(recoveryKey);
  assert.equal((await existingIdentity(id)).publicKeyText, legacy.publicKey);
});
test("a forgotten password is reset with the recovery key, which is then replaced", async () => {
  const server = fakeServer({ _id: "0123456789abcdef0123f00d", encryptionKey: null });
  let recoveryKey;
  const created = await pairLegacyIdentity(server.user, server.api, undefined, { onRecoveryKey: (key) => { recoveryKey = key; } });
  await forgetDeviceKey(server.user._id);
  await assert.rejects(recoverAccount(server.api, server.user.email, formatRecoveryKey(new Uint8Array(15)), "brand-new-password"));
  assert.equal(server.password, "password-1234");
  const result = await recoverAccount(server.api, server.user.email, recoveryKey, "brand-new-password");
  assert.equal(server.password, "brand-new-password");
  assert.equal(result.user.encryptionKey, created.encryptionKey);
  assert.equal((await existingIdentity(server.user._id)).publicKeyText, created.encryptionKey);
  assert.notEqual(result.recoveryKey, recoveryKey, "the proof was shown to the server, so a fresh key is issued");
  assert.equal(server.verifier, await recoveryVerifier(result.recoveryKey));
});
test("rotation keeps older messages readable on the original device", async () => {
  const me = "0123456789abcdef0123aaaa";
  const old = await identity(me);
  const server = fakeServer({ _id: me, encryptionKey: old.publicKeyText });
  const peer = { _id: "0123456789abcdef0123bbbb", encryptionKey: (await identity("0123456789abcdef0123bbbb")).publicKeyText };
  const before = { ...(await encrypt(me, peer, "before rotation")), senderId: me, receiverId: peer._id };
  const { user: rotated, recoveryKey } = await rotateIdentity(server.user, server.api, "password-1234");
  assert.ok(recoveryKey);
  assert.notEqual(rotated.encryptionKey, old.publicKeyText);
  assert.equal((await existingIdentity(me)).publicKeyText, rotated.encryptionKey);
  assert.equal((await decrypt(me, peer, before)).text, "before rotation");
  const after = { ...(await encrypt(me, peer, "after rotation")), senderId: me, receiverId: peer._id };
  assert.equal((await decrypt(me, peer, after)).text, "after rotation");
});
test("channel encryption binds channel identity and supports a sender's own envelope", async () => {
  const alice = {
    _id: "channel-alice",
    encryptionKey: (await identity("channel-alice")).publicKeyText,
  };
  const bob = {
    _id: "channel-bob",
    encryptionKey: (await identity("channel-bob")).publicKeyText,
  };
  const own = {
    ...(await encrypt(alice._id, alice, "hello channel", "channel-one")),
    senderId: alice._id,
    receiverId: alice._id,
    channelId: "channel-one",
  };
  assert.equal((await decrypt(alice._id, alice, own)).text, "hello channel");
  const message = {
    ...(await encrypt(alice._id, bob, "hello channel", "channel-one")),
    senderId: alice._id,
    receiverId: bob._id,
    channelId: "channel-one",
  };
  assert.equal((await decrypt(bob._id, alice, message)).text, "hello channel");
  assert.equal(
    (await decrypt(bob._id, alice, { ...message, channelId: "channel-two" }))
      .unreadable,
    true,
  );
  assert.equal(
    (await decrypt(bob._id, alice, { ...message, channelId: undefined }))
      .unreadable,
    true,
  );
});
test("identities persist and concurrent initialization shares a non-exportable private key", async () => {
  const [a, b] = await Promise.all([identity("alice"), identity("alice")]);
  assert.equal(a.publicKeyText, b.publicKeyText);
  assert.equal(a.privateKey.extractable, false);
  assert.match(a.publicKeyText, /^[A-Za-z0-9+/]{87}=$/);
  await assert.rejects(crypto.subtle.exportKey("jwk", a.privateKey));
});
test("both participants decrypt, envelopes are randomized, tampering fails closed", async () => {
  const alice = {
    _id: "alice",
    encryptionKey: (await identity("alice")).publicKeyText,
  };
  const bob = {
    _id: "bob",
    encryptionKey: (await identity("bob")).publicKeyText,
  };
  const secret = "private hello";
  const envelope = await encrypt("alice", bob, secret);
  const message = { ...envelope, senderId: "alice", receiverId: "bob" };
  assert.equal((await decrypt("bob", alice, message)).text, secret);
  assert.equal((await decrypt("alice", bob, message)).text, secret);
  assert.ok(!JSON.stringify(envelope).includes(secret));
  assert.notEqual((await encrypt("alice", bob, secret)).iv, envelope.iv);
  assert.equal(
    (await decrypt("bob", alice, { ...message, senderId: "mallory" }))
      .unreadable,
    true,
  );
  assert.equal(
    (await decrypt("bob", alice, { ...message, ciphertext: "AAAA" }))
      .unreadable,
    true,
  );
  assert.equal(
    (await fingerprint(alice.encryptionKey)).replaceAll(" ", "").length,
    64,
  );
});
test("remembered contact key changes block sending", async () => {
  const peer = {
    _id: "contact",
    encryptionKey: (await identity("contact")).publicKeyText,
  };
  await encrypt("alice", peer, "first");
  const replacement = (await identity("impostor")).publicKeyText;
  await assert.rejects(
    encrypt("alice", { ...peer, encryptionKey: replacement }, "blocked"),
    /Contact key changed/,
  );
});

test("key changes need an explicit safety-code confirmation and an unchanged stored key", async () => {
  const me = "verify-owner";
  const oldKey = (await identity("verify-old")).publicKeyText;
  const newKey = (await identity("verify-new")).publicKeyText;
  const peer = { _id: "verify-contact", encryptionKey: oldKey };
  await encrypt(me, peer, "before");
  const changed = { ...peer, encryptionKey: newKey };
  assert.deepEqual(await contactKeyStatus(me, changed), { saved: oldKey, changed: true });
  await assert.rejects(acceptVerifiedContactKey(me, changed, newKey), /no longer pending/);
  await assert.rejects(encrypt(me, changed, "blocked"), /Contact key changed/);
  await acceptVerifiedContactKey(me, changed, oldKey);
  assert.equal((await contactKeyStatus(me, changed)).changed, false);
  await assert.doesNotReject(encrypt(me, changed, "after"));
  const thirdKey = (await identity("verify-third")).publicKeyText;
  await assert.rejects(acceptVerifiedContactKey(me, { ...peer, encryptionKey: thirdKey }, oldKey), /Stored contact key changed/);
  await assert.rejects(encrypt(me, peer, "old key blocked"), /Contact key changed/);
});

test("separate client instances cannot overwrite the first identity or contact key", async () => {
  const first = await import("./crypto.js?client=first");
  const second = await import("./crypto.js?client=second");
  const [one, two] = await Promise.all([first.identity("concurrent-account"), second.identity("concurrent-account")]);
  assert.equal(one.publicKeyText, two.publicKeyText);
  const a = { _id: "concurrent-contact", encryptionKey: (await identity("contact-choice-a")).publicKeyText };
  const b = { ...a, encryptionKey: (await identity("contact-choice-b")).publicKeyText };
  const results = await Promise.allSettled([
    first.encrypt("concurrent-account", a, "message"),
    second.encrypt("concurrent-account", b, "message"),
  ]);
  assert.equal(results.filter((result) => result.status === "fulfilled").length, 1);
  assert.match(results.find((result) => result.status === "rejected").reason.message, /Contact key changed/);
});

test("invalid contact keys cannot poison first-use trust", async () => {
  const peer = { _id: "invalid-first-contact", encryptionKey: "AAAA" };
  await assert.rejects(encrypt("invalid-first-owner", peer, "message"));
  const valid = (await identity("valid-after-invalid")).publicKeyText;
  await assert.doesNotReject(encrypt("invalid-first-owner", { ...peer, encryptionKey: valid }, "message"));
});

test("reply text and target stay inside the encrypted envelope", async () => {
  const alice = { _id: "reply-alice", encryptionKey: (await identity("reply-alice")).publicKeyText };
  const bob = { _id: "reply-bob", encryptionKey: (await identity("reply-bob")).publicKeyText };
  const target = "0123456789abcdef01234567";
  const envelope = await encrypt(alice._id, bob, encodeMessage("private reply", target));
  assert.ok(!JSON.stringify(envelope).includes(target));
  assert.ok(!JSON.stringify(envelope).includes("private reply"));
  const decoded = await decrypt(bob._id, alice, { ...envelope, senderId: alice._id, receiverId: bob._id });
  assert.equal(decoded.replyTo, target);
  assert.equal(decoded.text, "private reply");
});

test("reaction and target stay inside the encrypted envelope", async () => {
  const alice = { _id: "reaction-alice", encryptionKey: (await identity("reaction-alice")).publicKeyText };
  const bob = { _id: "reaction-bob", encryptionKey: (await identity("reaction-bob")).publicKeyText };
  const reaction = { target: "0123456789abcdef01234567", emoji: REACTION_EMOJI[0], action: "add" };
  const envelope = await encrypt(alice._id, bob, encodeReaction(reaction));
  assert.ok(!JSON.stringify(envelope).includes(reaction.target));
  assert.ok(!JSON.stringify(envelope).includes(reaction.emoji));
  const decoded = await decrypt(bob._id, alice, { ...envelope, senderId: alice._id, receiverId: bob._id });
  assert.deepEqual(decoded.reaction, reaction);
});
