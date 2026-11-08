// Regenerates key-backup-fixture.json from the web client's implementation.
// Run from the repository root: node desktop/tests/generate-key-backup-fixture.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { wrapPrivateKey, unwrapPrivateKey } from "../../frontend/src/lib/keyBackup.js";
const web = JSON.parse(readFileSync(new URL("./webcrypto-fixture.json", import.meta.url)));
const password = "correct horse battery staple ✓";
const accountId = "0123456789abcdef01234567";
const pkcs8 = Uint8Array.from(Buffer.from(web.alicePrivate, "base64"));
const salt = Uint8Array.from({ length: 16 }, (_, i) => i + 1);
const iv = Uint8Array.from({ length: 12 }, (_, i) => 0xa0 + i);
const backup = await wrapPrivateKey(pkcs8, password, accountId, web.alicePublic, { salt, iv });
const back = await unwrapPrivateKey(backup, password, accountId);
if (Buffer.from(back).toString("base64") !== web.alicePrivate) throw new Error("round trip failed");
writeFileSync(new URL("./key-backup-fixture.json", import.meta.url), JSON.stringify({ password, accountId, pkcs8: web.alicePrivate, backup }, null, 2) + "\n");
console.log("wrote key-backup-fixture.json");
