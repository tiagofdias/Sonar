import { axiosInstance as api } from "./axios";

export const MAX_FILE_BYTES = 8 * 1024 * 1024;
const AAD = new TextEncoder().encode("sonar-file-v1");

const toBase64 = (bytes) => btoa(String.fromCharCode(...bytes));
const fromBase64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

export const formatBytes = (size) => (size < 1024 ? `${size} B` : size < 1048576 ? `${(size / 1024).toFixed(0)} KB` : `${(size / 1048576).toFixed(1)} MB`);

// Encrypts on this device with a new key per file, then uploads only ciphertext.
// `target` is { to: userId } or { community: communityId }.
export async function uploadFile(file, target) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const raw = crypto.getRandomValues(new Uint8Array(32));
  const key = await crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt"]);
  const ciphertext = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: AAD }, key, await file.arrayBuffer());
  const { data } = await api.post("/attachments", ciphertext, { params: target, headers: { "Content-Type": "application/octet-stream" } });
  return { id: data.id, key: toBase64(raw), iv: toBase64(iv), name: file.name.slice(0, 255) || "file", type: (file.type || "application/octet-stream").slice(0, 120), size: file.size };
}

export async function saveFile(meta) {
  const { data } = await api.get(`/attachments/${meta.id}`, { responseType: "arraybuffer" });
  const key = await crypto.subtle.importKey("raw", fromBase64(meta.key), "AES-GCM", false, ["decrypt"]);
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: fromBase64(meta.iv), additionalData: AAD }, key, data);
  // Saved as a plain download: a shared file must never run or render inside Sonar.
  const url = URL.createObjectURL(new Blob([plain], { type: "application/octet-stream" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = meta.name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
}

export const deleteFile = (id) => api.delete(`/attachments/${id}`).catch(() => {});
