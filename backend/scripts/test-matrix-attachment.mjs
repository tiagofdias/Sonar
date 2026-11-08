// Synthetic test vectors only; no user files or production services.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { randomBytes } from "node:crypto";
import { encryptMatrixAttachment, decryptMatrixAttachment, MAX_MATRIX_ATTACHMENT_BYTES } from "../../frontend/src/lib/matrixAttachment.js";

const executable = resolve("../desktop/target/debug/examples/matrix_attachment_probe.exe");
function native(request, expectSuccess = true) {
  const result = spawnSync(executable, [], { input: JSON.stringify(request), encoding: "utf8", windowsHide: true });
  if (expectSuccess) { assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout); }
  assert.notEqual(result.status, 0);
}
const plaintext = randomBytes(4097);
const browser = encryptMatrixAttachment(plaintext);
const ciphertext = Buffer.from(browser.ciphertext), metadata = browser.metadata;
assert.notDeepEqual(ciphertext, plaintext);
assert.deepEqual(Buffer.from(native({ action: "decrypt", data: ciphertext.toString("base64"), metadata }).data, "base64"), plaintext);
const rust = native({ action: "encrypt", data: plaintext.toString("base64") });
assert.deepEqual(Buffer.from(decryptMatrixAttachment(Buffer.from(rust.data, "base64"), rust.metadata)), plaintext);
const altered = Buffer.from(ciphertext); altered[0] ^= 1;
native({ action: "decrypt", data: altered.toString("base64"), metadata }, false);
const corrupt = Buffer.from(rust.data, "base64"); corrupt[0] ^= 1;
assert.throws(() => decryptMatrixAttachment(corrupt, rust.metadata));
assert.throws(() => encryptMatrixAttachment(new Uint8Array(0)));
assert.throws(() => encryptMatrixAttachment(new Uint8Array(MAX_MATRIX_ATTACHMENT_BYTES + 1)));
console.log("PASS: Matrix attachment encryption interoperates between Rust and WebAssembly and rejects tampering.");
