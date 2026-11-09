import { test } from "node:test";
import assert from "node:assert/strict";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import { MongoMemoryServer } from "mongodb-memory-server";
import { app, server } from "../lib/socket.js";
import authRoutes from "./auth.route.js";
import User from "../models/user.model.js";
import { ensureUsernames } from "../lib/usernames.js";
import {
  createBackedUpIdentity,
  createRecoverableIdentity,
  newRecovery,
  recoveryProof,
  rewrapBackup,
  unwrapPrivateKey,
  unwrapWithRecovery,
} from "../../../frontend/src/lib/keyBackup.js";

test("encrypted key backups bind to the account key and follow password changes", async (t) => {
  const database = await MongoMemoryServer.create({ instance: { ip: "127.0.0.1" } });
  try {
    await mongoose.connect(database.getUri("sonar-key-backup-tests"));
    app.use(express.json());
    app.use(cookieParser());
    app.use("/api/auth", authRoutes);
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${server.address().port}`;
    async function request(path, method = "GET", body, cookie) {
      const res = await fetch(url + "/api/auth" + path, {
        method,
        headers: { "Content-Type": "application/json", ...(cookie ? { Cookie: cookie } : {}) },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: res.status, body: await res.json(), cookie: res.headers.get("set-cookie")?.split(";")[0] };
    }
    const password = "first-device-password";
    const signup = await request("/signup", "POST", { fullName: "Backup tester", email: "backup@example.invalid", password });
    assert.equal(signup.status, 201);
    let cookie = signup.cookie;
    const accountId = signup.body._id;
    const first = await createBackedUpIdentity(password, accountId);

    await t.test("backups are validated and stored with the first key", async () => {
      assert.deepEqual((await request("/key-backup", "GET", undefined, cookie)).body, { backup: null });
      const wrongKey = { ...first.backup, publicKey: (await createBackedUpIdentity(password, accountId)).publicKey };
      assert.equal((await request("/encryption-key", "PUT", { publicKey: first.publicKey, backup: wrongKey }, cookie)).status, 400);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: first.publicKey, backup: { ...first.backup, iterations: 1000 } }, cookie)).status, 400);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: first.publicKey, backup: { ...first.backup, salt: "AAAA" } }, cookie)).status, 400);
      const stored = await request("/encryption-key", "PUT", { publicKey: first.publicKey, backup: first.backup }, cookie);
      assert.equal(stored.status, 200);
      assert.equal(stored.body.keyBackup, undefined);
      assert.equal((await request("/check", "GET", undefined, cookie)).body.keyBackup, undefined);
      const fetched = (await request("/key-backup", "GET", undefined, cookie)).body.backup;
      assert.deepEqual(fetched, first.backup);
      // Re-binding the same key works, but a session alone cannot swap the backup.
      assert.equal((await request("/encryption-key", "PUT", { publicKey: first.publicKey }, cookie)).status, 200);
      const forged = await createBackedUpIdentity("attacker-chosen-password", accountId);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: first.publicKey, backup: { ...forged.backup, publicKey: first.publicKey } }, cookie)).status, 409);
      assert.deepEqual((await request("/key-backup", "GET", undefined, cookie)).body.backup, first.backup);
      assert.ok((await unwrapPrivateKey(fetched, password, accountId)).length > 60);
      assert.equal((await request("/key-backup")).status, 401);
    });

    await t.test("an existing key cannot be replaced without the password", async () => {
      const other = await createBackedUpIdentity(password, accountId);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: other.publicKey, backup: other.backup }, cookie)).status, 409);
      assert.equal((await request("/encryption-key/rotate", "POST", { currentPassword: "not-the-password", previousKey: first.publicKey, publicKey: other.publicKey, backup: other.backup }, cookie)).status, 400);
      assert.equal((await request("/encryption-key/rotate", "POST", { currentPassword: password, previousKey: other.publicKey, publicKey: other.publicKey, backup: other.backup }, cookie)).status, 409);
      assert.equal((await User.findById(accountId)).encryptionKey, first.publicKey);
    });

    await t.test("password changes must carry the re-protected backup", async () => {
      const nextPassword = "second-device-password";
      assert.equal((await request("/password", "PUT", { currentPassword: password, newPassword: nextPassword }, cookie)).status, 400);
      assert.equal((await User.findById(accountId)).sessionVersion, 0);
      const backup = (await request("/key-backup", "GET", undefined, cookie)).body.backup;
      const keyBackup = await rewrapBackup(backup, password, nextPassword, accountId);
      const changed = await request("/password", "PUT", { currentPassword: password, newPassword: nextPassword, keyBackup }, cookie);
      assert.equal(changed.status, 200);
      cookie = (await request("/login", "POST", { email: "backup@example.invalid", password: nextPassword })).cookie;
      const after = (await request("/key-backup", "GET", undefined, cookie)).body.backup;
      assert.deepEqual(after, keyBackup);
      await assert.rejects(unwrapPrivateKey(after, password, accountId));
      assert.ok((await unwrapPrivateKey(after, nextPassword, accountId)).length > 60);

      const rotated = await createBackedUpIdentity(nextPassword, accountId);
      const result = await request("/encryption-key/rotate", "POST", { currentPassword: nextPassword, previousKey: first.publicKey, publicKey: rotated.publicKey, backup: rotated.backup }, cookie);
      assert.equal(result.status, 200);
      assert.equal(result.body.encryptionKey, rotated.publicKey);
      assert.deepEqual((await request("/key-backup", "GET", undefined, cookie)).body.backup, rotated.backup);
    });
    await t.test("status can be chosen, is validated and survives sign-in", async () => {
      const email = "status@example.invalid";
      const made = await request("/signup", "POST", { fullName: "Status tester", email, password: "status-password-1" });
      assert.equal(made.body.presence, "online");
      assert.equal((await request("/presence", "PUT", { presence: "invisible" }, made.cookie)).status, 400);
      assert.equal((await request("/presence", "PUT", { presence: "busy" })).status, 401);
      assert.equal((await request("/presence", "PUT", { presence: "busy" }, made.cookie)).body.presence, "busy");
      assert.equal((await request("/login", "POST", { email, password: "status-password-1" })).body.presence, "busy");
    });
    await t.test("usernames are unique, validated and editable", async () => {
      const a = await request("/signup", "POST", { fullName: "Ana Lopes", username: "@Ana.Lopes", email: "ana@example.invalid", password: "ana-password-1" });
      assert.equal(a.status, 201);
      assert.equal(a.body.username, "ana.lopes");
      assert.equal((await request("/signup", "POST", { fullName: "Other", username: "ana.lopes", email: "other@example.invalid", password: "other-password-1" })).status, 400);
      assert.equal((await request("/signup", "POST", { fullName: "Bad", username: "no spaces", email: "bad@example.invalid", password: "bad-password-12" })).status, 400);
      const auto = await request("/signup", "POST", { fullName: "Ána Lopes", email: "auto@example.invalid", password: "auto-password-1" });
      assert.equal(auto.body.username, "analopes");
      assert.equal((await request("/update-profile", "PUT", { fullName: "Ana", username: "analopes" }, a.cookie)).status, 409);
      const renamed = await request("/update-profile", "PUT", { fullName: "Ana", username: "ana_new" }, a.cookie);
      assert.equal(renamed.body.username, "ana_new");
      assert.equal((await request("/check", "GET", undefined, a.cookie)).body.username, "ana_new");
      await User.updateOne({ email: "auto@example.invalid" }, { $unset: { username: 1 } });
      await ensureUsernames();
      assert.equal((await User.findOne({ email: "auto@example.invalid" })).username, "analopes");
    });
    await t.test("recovery keys replace password backups and reset a forgotten password", async () => {
      const email = "recovery@example.invalid";
      const signup = await request("/signup", "POST", { fullName: "Recovery tester", email, password: "original-password" });
      let session = signup.cookie;
      const id = signup.body._id;
      const created = await createRecoverableIdentity(id);
      assert.equal((await request("/encryption-key", "PUT", { publicKey: created.publicKey, backup: created.backup }, session)).status, 400, "a recovery backup needs its verifier");
      assert.equal((await request("/encryption-key", "PUT", { publicKey: created.publicKey, backup: created.backup, recoveryVerifier: created.verifier }, session)).status, 200);
      const stored = (await request("/key-backup", "GET", undefined, session)).body.backup;
      assert.deepEqual(stored, created.backup);
      assert.ok(await unwrapWithRecovery(stored, created.recoveryKey, id));
      // A password change no longer touches a recovery backup.
      assert.equal((await request("/password", "PUT", { currentPassword: "original-password", newPassword: "second-password" }, session)).status, 200);
      session = (await request("/login", "POST", { email, password: "second-password" })).cookie;
      assert.deepEqual((await request("/key-backup", "GET", undefined, session)).body.backup, created.backup);
      // Replacing the recovery key needs the password.
      const pkcs8 = await unwrapWithRecovery(stored, created.recoveryKey, id);
      const next = await newRecovery(pkcs8, id, created.publicKey);
      assert.equal((await request("/recovery", "PUT", { currentPassword: "wrong-password", backup: next.backup, recoveryVerifier: next.verifier }, session)).status, 400);
      assert.equal((await request("/recovery", "PUT", { currentPassword: "second-password", backup: next.backup, recoveryVerifier: next.verifier }, session)).status, 200);
      // Forgotten password: the old recovery key no longer works, the new one does.
      const reset = (proofKey, extra = {}) => recoveryProof(proofKey).then((proof) => request("/recover", "POST", { email, proof, newPassword: "third-password-1", ...extra }));
      assert.equal((await reset(created.recoveryKey)).status, 400);
      assert.equal((await reset(next.recoveryKey, { email: "nobody@example.invalid" })).status, 400);
      assert.equal((await reset(next.recoveryKey, { newPassword: "short" })).status, 400);
      const recovered = await reset(next.recoveryKey);
      assert.equal(recovered.status, 200);
      assert.equal(recovered.body.encryptionKey, created.publicKey);
      assert.ok(recovered.cookie);
      assert.equal((await request("/check", "GET", undefined, session)).status, 401, "older sessions are signed out");
      assert.equal((await request("/login", "POST", { email, password: "third-password-1" })).status, 200);
      const user = await User.findById(id).select("+recoveryVerifier +keyBackup");
      assert.equal(user.recoveryVerifier, next.verifier);
      assert.ok(!JSON.stringify(user).includes(next.recoveryKey));
    });
  } finally {
    server.closeAllConnections?.();
    await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
