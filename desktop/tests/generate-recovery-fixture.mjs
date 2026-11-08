// Regenerates recovery-fixture.json from the web client's implementation.
// Run from the repository root: node desktop/tests/generate-recovery-fixture.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { formatRecoveryKey, recoveryProof, recoveryVerifier, wrapWithRecovery, unwrapWithRecovery } from "../../frontend/src/lib/keyBackup.js";
const web = JSON.parse(readFileSync(new URL("./webcrypto-fixture.json", import.meta.url)));
const accountId = "0123456789abcdef01234567";
const recoveryKey = formatRecoveryKey(Uint8Array.from({ length: 15 }, (_, i) => i * 17 + 3));
const pkcs8 = Uint8Array.from(Buffer.from(web.alicePrivate, "base64"));
const iv = Uint8Array.from({ length: 12 }, (_, i) => 0x40 + i);
const backup = await wrapWithRecovery(pkcs8, recoveryKey, accountId, web.alicePublic, { iv });
if (Buffer.from(await unwrapWithRecovery(backup, recoveryKey, accountId)).toString("base64") !== web.alicePrivate) throw new Error("round trip failed");
writeFileSync(new URL("./recovery-fixture.json", import.meta.url), JSON.stringify({ accountId, recoveryKey, pkcs8: web.alicePrivate, proof: await recoveryProof(recoveryKey), verifier: await recoveryVerifier(recoveryKey), backup }, null, 2) + "\n");
console.log("wrote recovery-fixture.json");
