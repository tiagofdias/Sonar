import { axiosInstance as api } from "./axios";

const bytes = (value) => Uint8Array.from(atob(value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=")), (c) => c.charCodeAt(0));
export const pushSupported = () => "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;

async function registration() {
  return navigator.serviceWorker.register("/sw.js");
}

export async function pushEnabled() {
  if (!pushSupported()) return false;
  try { return Boolean(await (await registration()).pushManager.getSubscription()); } catch { return false; }
}

export async function enablePush() {
  if ((await Notification.requestPermission()) !== "granted") throw new Error("Allow notifications for this site first.");
  const worker = await registration();
  await navigator.serviceWorker.ready;
  const { publicKey } = (await api.get("/preferences/push-key")).data;
  const subscription = await worker.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: bytes(publicKey) });
  await api.post("/preferences/push", subscription.toJSON());
}

export async function disablePush() {
  const subscription = await (await registration()).pushManager.getSubscription();
  if (!subscription) return;
  await api.delete("/preferences/push", { data: { endpoint: subscription.endpoint } });
  await subscription.unsubscribe();
}
