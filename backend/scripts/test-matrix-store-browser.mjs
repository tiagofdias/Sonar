// Runs against a temporary browser profile and Vite only; no real database.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const requireFrontend = createRequire(resolve(root, "frontend/package.json"));
const { createServer } = await import(pathToFileURL(requireFrontend.resolve("vite")).href);
let vite, browser;
try {
  vite = await createServer({ root: resolve(root, "frontend"), configFile: resolve(root, "frontend/vite.config.js"), server: { host: "127.0.0.1", port: 0, proxy: {} } });
  await vite.listen();
  const port = vite.httpServer.address().port;
  browser = await chromium.launch({ headless: true, ...(process.env.SONAR_BROWSER_EXECUTABLE ? { executablePath: process.env.SONAR_BROWSER_EXECUTABLE } : {}) });
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/`);
  const result = await page.evaluate(async () => {
    const { createMatrixDeviceStore, withMatrixDeviceStore } = await import("/src/lib/matrixStore.js");
    const key = crypto.getRandomValues(new Uint8Array(32));
    const descriptor = await createMatrixDeviceStore("@0123456789abcdef01234567:sonar.invalid", "TESTDEVICE", key);
    const getPublicKey = machine => {
      const keys = machine.identityKeys, ed = keys.ed25519;
      try { return ed.toBase64(); } finally { ed.free(); keys.free(); }
    };
    const first = await withMatrixDeviceStore(descriptor, key, getPublicKey);
    const second = await withMatrixDeviceStore(descriptor, key, getPublicKey);
    let wrongKeyRejected = false, wrongIdentityRejected = false, callbackRan = false;
    try { await withMatrixDeviceStore(descriptor, crypto.getRandomValues(new Uint8Array(32)), getPublicKey); } catch { wrongKeyRejected = true; }
    try { await withMatrixDeviceStore({ ...descriptor, ed25519: "A".repeat(43) }, key, () => { callbackRan = true; }); } catch { wrongIdentityRejected = true; }
    const parallel = await Promise.all(Array.from({ length: 2 }, () => withMatrixDeviceStore(descriptor, key, getPublicKey)));
    return { descriptor, key: Array.from(key), first, second, wrongKeyRejected, wrongIdentityRejected, callbackRan, parallel };
  });
  assert.equal(result.first, result.descriptor.ed25519);
  assert.equal(result.second, result.first);
  assert.deepEqual(result.parallel, [result.first, result.first]);
  assert.equal(result.wrongKeyRejected, true);
  assert.equal(result.wrongIdentityRejected, true);
  assert.equal(result.callbackRan, false);
  const vault = await page.evaluate(async () => {
    const { createBrowserMatrixIdentity, withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const user = "@aaaaaaaaaaaaaaaaaaaaaaaa:sonar.invalid";
    let missingRejected = false;
    try { await withBrowserMatrixIdentity(user, () => {}); } catch { missingRejected = true; }
    const attempts = await Promise.allSettled([createBrowserMatrixIdentity(user, "VAULTONE"), createBrowserMatrixIdentity(user, "VAULTTWO")]);
    const succeeded = attempts.filter(x => x.status === "fulfilled");
    const identity = await withBrowserMatrixIdentity(user, (machine, descriptor) => {
      const keys = machine.identityKeys, key = keys.ed25519;
      try { return { descriptor, publicKey: key.toBase64() }; } finally { key.free(); keys.free(); }
    });
    const db = await new Promise((resolve, reject) => { const request = indexedDB.open("sonar-matrix-identity-v1"); request.onsuccess = () => resolve(request.result); request.onerror = () => reject(request.error); });
    const record = await new Promise((resolve, reject) => { const request = db.transaction("identities").objectStore("identities").get(user); request.onsuccess = () => resolve(request.result); request.onerror = () => reject(request.error); });
    let exportRejected = false;
    try { await crypto.subtle.exportKey("raw", record.wrappingKey); } catch { exportRejected = true; }
    db.close();
    return { missingRejected, count: succeeded.length, identity, exportRejected };
  });
  assert.equal(vault.missingRejected, true);
  assert.equal(vault.count, 1);
  assert.equal(vault.identity.publicKey, vault.identity.descriptor.ed25519);
  assert.equal(vault.exportRejected, true);
  const rotation = await page.evaluate(async user => {
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { ensureMatrixRoomRevision } = await import("/src/lib/matrixRoomRevision.js");
    const sdk = await import("/src/lib/matrixRoomRevision.browser-fixture.js");
    return withBrowserMatrixIdentity(user, async (machine, descriptor) => {
      const roomId = "!browser-rotation:sonar.invalid", room = new sdk.RoomId(roomId);
      const settings = new sdk.EncryptionSettings(); settings.sharingStrategy = sdk.CollectStrategy.allDevices();
      try {
        await ensureMatrixRoomRevision(sdk, machine, descriptor.storeName, roomId, 0);
        await machine.shareRoomKey(room, [], settings);
        const first = JSON.parse(await machine.encryptRoomEvent(room, "m.room.message", JSON.stringify({ msgtype: "m.text", body: "one" })));
        await ensureMatrixRoomRevision(sdk, machine, descriptor.storeName, roomId, 1);
        await machine.shareRoomKey(room, [], settings);
        const second = JSON.parse(await machine.encryptRoomEvent(room, "m.room.message", JSON.stringify({ msgtype: "m.text", body: "two" })));
        return { first: first.session_id, second: second.session_id };
      } finally { settings.free(); room.free(); }
    });
  }, vault.identity.descriptor.userId);
  assert.notEqual(rotation.first, rotation.second);
  await page.reload();
  const vaultReopened = await page.evaluate(async user => {
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    return withBrowserMatrixIdentity(user, (_machine, descriptor) => descriptor);
  }, vault.identity.descriptor.userId);
  assert.deepEqual(vaultReopened, vault.identity.descriptor);
  const rollbackBlocked = await page.evaluate(async user => {
    const { withBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    const { ensureMatrixRoomRevision } = await import("/src/lib/matrixRoomRevision.js");
    const sdk = await import("/src/lib/matrixRoomRevision.browser-fixture.js");
    try { await withBrowserMatrixIdentity(user, (machine, descriptor) => ensureMatrixRoomRevision(sdk, machine, descriptor.storeName, "!browser-rotation:sonar.invalid", 0)); }
    catch { return true; }
    return false;
  }, vault.identity.descriptor.userId);
  assert.equal(rollbackBlocked, true);
  const tampered = await page.evaluate(async user => {
    const db = await new Promise((resolve, reject) => { const r = indexedDB.open("sonar-matrix-identity-v1"); r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error); });
    await new Promise((resolve, reject) => {
      const tx = db.transaction("identities", "readwrite"), store = tx.objectStore("identities"), r = store.get(user);
      r.onsuccess = () => { const record = r.result; record.descriptor.ed25519 = "A".repeat(43); store.put(record, user); };
      tx.oncomplete = resolve; tx.onabort = () => reject(tx.error);
    });
    db.close();
    const { withBrowserMatrixIdentity, createBrowserMatrixIdentity } = await import("/src/lib/matrixIdentityVault.js");
    let ran = false, rejected = false, replacementRejected = false;
    try { await withBrowserMatrixIdentity(user, () => { ran = true; }); } catch { rejected = true; }
    try { await createBrowserMatrixIdentity(user, "REPLACEMENT"); } catch { replacementRejected = true; }
    return { ran, rejected, replacementRejected };
  }, vault.identity.descriptor.userId);
  assert.deepEqual(tampered, { ran: false, rejected: true, replacementRejected: true });
  const reopened = await page.evaluate(async ({ descriptor, key }) => {
    const { withMatrixDeviceStore } = await import("/src/lib/matrixStore.js");
    return withMatrixDeviceStore(descriptor, new Uint8Array(key), machine => {
      const keys = machine.identityKeys, ed = keys.ed25519;
      try { return ed.toBase64(); } finally { ed.free(); keys.free(); }
    });
  }, { descriptor: result.descriptor, key: result.key });
  assert.equal(reopened, result.first);
  const otherPage = await context.newPage();
  await otherPage.goto(`http://127.0.0.1:${port}/`);
  const queryKey = ({ descriptor, key }) => import("/src/lib/matrixStore.js").then(({ withMatrixDeviceStore }) => withMatrixDeviceStore(descriptor, new Uint8Array(key), async machine => {
    await new Promise(resolve => setTimeout(resolve, 50));
    const keys = machine.identityKeys, ed = keys.ed25519;
    try { return ed.toBase64(); } finally { ed.free(); keys.free(); }
  }));
  assert.deepEqual(await Promise.all([page.evaluate(queryKey, { descriptor: result.descriptor, key: result.key }), otherPage.evaluate(queryKey, { descriptor: result.descriptor, key: result.key })]), [result.first, result.first]);
  await otherPage.close();
  const lost = await page.evaluate(async ({ descriptor, key }) => {
    const names = (await indexedDB.databases()).map(db => db.name).filter(name => name?.includes(descriptor.storeName));
    if (!names.length) throw new Error("Matrix store databases not found");
    for (const name of names) await new Promise((resolve, reject) => {
      const request = indexedDB.deleteDatabase(name);
      request.onsuccess = resolve;
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error("Matrix database deletion blocked"));
    });
    const { withMatrixDeviceStore } = await import("/src/lib/matrixStore.js");
    let callbackRan = false, rejected = false;
    try { await withMatrixDeviceStore(descriptor, new Uint8Array(key), () => { callbackRan = true; }); } catch { rejected = true; }
    return { rejected, callbackRan };
  }, { descriptor: result.descriptor, key: result.key });
  assert.deepEqual(lost, { rejected: true, callbackRan: false });
  const queued = await page.evaluate(async () => {
    const { queueVerification, deliverQueuedVerification } = await import("/src/lib/matrixVerificationOutbox.js");
    const userId = "@0123456789abcdef01234567:sonar.invalid", deviceId = "TESTDEVICE";
    const sdk = { RequestType: { ToDevice: 2 }, UserId: class { constructor(value) { this.value = value; } }, DeviceId: class { constructor(value) { this.value = value; } } };
    let encryptions = 0;
    const machine = { async getDevice() { return { isDeleted: () => false, isBlacklisted: () => false, free() {}, async encryptToDeviceEvent() { encryptions++; return JSON.stringify({ algorithm: "m.olm.v1.curve25519-aes-sha2", ciphertext: { synthetic: { type: 0, body: "encrypted" } }, sender_key: "synthetic" }); } }; }, async markRequestAsSent() { throw new Error("must not acknowledge failed send"); } };
    const request = { id: "browser-outbox-test", txn_id: "browser-outbox-test", type: 2, event_type: "m.key.verification.request", body: JSON.stringify({ messages: { [userId]: { [deviceId]: { transaction_id: "synthetic" } } } }) };
    const name = `sonar-matrix-v2-${"a".repeat(32)}`;
    const first = await queueVerification(sdk, machine, name, userId, deviceId, request);
    const again = await queueVerification(sdk, machine, name, userId, deviceId, request);
    let failed = false;
    try { await deliverQueuedVerification(sdk, machine, name, request.id, async () => { throw new Error("offline"); }); } catch { failed = true; }
    return { first, again, encryptions, failed, name };
  });
  assert.deepEqual(queued.first, queued.again);
  assert.equal(queued.encryptions, 1);
  assert.equal(queued.failed, true);
  await page.reload();
  const retried = await page.evaluate(async ({ name, first }) => {
    const { deliverQueuedVerification } = await import("/src/lib/matrixVerificationOutbox.js");
    let sent, acknowledged = false;
    const sdk = { RequestType: { ToDevice: 2 } };
    const machine = { async markRequestAsSent() { acknowledged = true; } };
    await deliverQueuedVerification(sdk, machine, name, first.requestId, async value => { sent = value; return true; });
    let gone = false;
    try { await deliverQueuedVerification(sdk, machine, name, first.requestId, async () => true); } catch { gone = true; }
    return { sent, acknowledged, gone };
  }, queued);
  assert.deepEqual(retried.sent, queued.first);
  assert.equal(retried.acknowledged, true);
  assert.equal(retried.gone, true);
  console.log("PASS: encrypted verification delivery survives a browser reload and retries the same ciphertext.");
  console.log("PASS: Matrix IndexedDB store persists its identity, rejects wrong keys and serializes access.");
} finally {
  await browser?.close();
  await vite?.close();
}
