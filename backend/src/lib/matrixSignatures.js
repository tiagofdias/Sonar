import { createPublicKey, verify } from "node:crypto";

function decode(value, length) {
  if (typeof value !== "string" || value.length !== Math.ceil(length * 8 / 6) || !/^[A-Za-z0-9+/]+$/.test(value)) return null;
  const bytes = Buffer.from(value, "base64");
  return bytes.length === length && bytes.toString("base64").replace(/=+$/, "") === value ? bytes : null;
}
export function verifyMatrixEd25519(key, signature, message) {
  const rawKey = decode(key, 32), rawSignature = decode(signature, 64);
  if (!rawKey || !rawSignature || typeof message !== "string") return false;
  try {
    const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), rawKey]), format: "der", type: "spki" });
    return verify(null, Buffer.from(message, "utf8"), publicKey, rawSignature);
  } catch { return false; }
}
