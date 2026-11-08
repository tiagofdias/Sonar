import { useState } from "react";
import { axiosInstance as api } from "../lib/axios";
import ExperimentalVerification from "./ExperimentalVerification";

// This opt-in UI is for isolated development only. It does not claim that
// Matrix messaging, device verification, or key recovery is ready.
export default function ExperimentalDeviceSettings({ accountId, onBusyChange, allowFirstDevice = true }) {
  const [device, setDevice] = useState(null);
  const [busy, setBusy] = useState(false);
  const [verificationActive, setVerificationActive] = useState(false);
  const [verificationBusy, setVerificationBusy] = useState(false);
  const [devices, setDevices] = useState(null), [removing, setRemoving] = useState(null);
  const [error, setError] = useState("");
  const run = async action => {
    if (busy || verificationActive || verificationBusy) return;
    setBusy(true); onBusyChange(true); setError("");
    try {
      const client = await import("../lib/matrixDeviceClient.js");
      if (action === "prepare") setDevice(await client.prepareBrowserMatrixDevice(accountId, api));
      else if (action === "bootstrap" && device) setDevice(await client.bootstrapExistingBrowserMatrixIdentity(device.userId, api));
      else if (action === "list" && device) { setDevices(await client.listBrowserMatrixDevices(device.userId, api)); setRemoving(null); }
      else if (action === "remove" && device && removing) { await client.revokeBrowserMatrixDevice(device.userId, removing, api); setDevices(await client.listBrowserMatrixDevices(device.userId, api)); setRemoving(null); }
    } catch (failure) {
      setError(failure.response?.data?.message || failure.message || "Device setup failed.");
    } finally { setBusy(false); onBusyChange(false); }
  };
  return (
    <section className="settings-retention" aria-labelledby="experimental-devices-title">
      <h4 id="experimental-devices-title">Experimental device setup</h4>
      <p>For isolated testing only. This prepares a separate Matrix identity without changing your existing browser key. Matrix messages and recovery are not ready.</p>
      <button type="button" className="secondary" disabled={busy || verificationActive || verificationBusy} onClick={() => run("prepare")}>
        {busy ? "Working..." : "Prepare this browser"}
      </button>
      {device && <p role="status">Device {device.deviceId}: {device.status === "authorized" ? "authorized on the server" : "awaiting authorization"}.</p>}
      {allowFirstDevice && device && device.status !== "authorized" && <>
        <p>Only use this first-device action in the original browser that holds your existing encryption key. It does not transfer message history.</p>
        <button type="button" className="secondary" disabled={busy || verificationActive || verificationBusy} onClick={() => run("bootstrap")}>Authorize this first device</button>
      </>}
      {error && <p role="alert">{error}</p>}
      {device && <>
        <button type="button" className="secondary" disabled={busy || verificationActive || verificationBusy} onClick={() => run("list")}>Show devices</button>
        {devices && <div><p>Server access status is separate from cryptographic verification.</p><ul>
          {devices.map(item => <li key={item.deviceId}><strong>{item.deviceId}</strong> {item.current ? "(this browser)" : ""} — {item.serverAuthorization}<br /><code>{item.ed25519}</code>{!item.current && devices.find(value => value.current)?.serverAuthorization === "authorized" && <button type="button" className="secondary" disabled={busy || verificationActive || verificationBusy} onClick={() => setRemoving(item)}>Remove {item.deviceId}</button>}</li>)}
        </ul></div>}
        {removing && <div role="alert"><p>Remove device {removing.deviceId}? Its server access will end. Copies already on that device cannot be erased remotely.</p><button type="button" className="secondary" disabled={busy || verificationActive || verificationBusy} onClick={() => run("remove")}>Confirm removal</button><button type="button" className="secondary" disabled={busy} onClick={() => setRemoving(null)}>Keep device</button></div>}
        <ExperimentalVerification userId={device.userId} onBusyChange={value => { setVerificationBusy(value); onBusyChange(value); }} onSessionChange={setVerificationActive} />
      </>}
    </section>
  );
}
