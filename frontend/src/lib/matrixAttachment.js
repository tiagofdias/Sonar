// Experimental Matrix attachment cryptography. Metadata contains the secret
// key and must only travel inside an authenticated encrypted message.
import { Attachment, EncryptedAttachment } from "@matrix-org/matrix-sdk-crypto-wasm";

export const MAX_MATRIX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
function checkBytes(data) {
  if (!(data instanceof Uint8Array) || data.byteLength === 0 || data.byteLength > MAX_MATRIX_ATTACHMENT_BYTES) throw new Error("Attachment size is invalid.");
}
export function encryptMatrixAttachment(data) {
  checkBytes(data);
  const encrypted = Attachment.encrypt(data);
  try {
    const metadata = encrypted.mediaEncryptionInfo;
    if (typeof metadata !== "string" || metadata.length > 4096) throw new Error("Attachment encryption failed.");
    return { ciphertext: encrypted.encryptedData, metadata };
  } finally { encrypted.free(); }
}
export function decryptMatrixAttachment(ciphertext, metadata) {
  checkBytes(ciphertext);
  if (typeof metadata !== "string" || metadata.length > 4096) throw new Error("Attachment metadata is invalid.");
  const encrypted = new EncryptedAttachment(ciphertext, metadata);
  try { return Attachment.decrypt(encrypted); }
  finally { encrypted.free(); }
}
