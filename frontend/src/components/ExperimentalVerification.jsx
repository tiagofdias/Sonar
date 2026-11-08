import { useEffect, useRef, useState } from "react";
import { axiosInstance as api } from "../lib/axios";

// Development-only, deliberately explicit steps. The SDK, not a server flag,
// decides whether the two devices have completed SAS verification.
export default function ExperimentalVerification({ userId, onBusyChange, onSessionChange }) {
  const [session, setSession] = useState(null), [flow, setFlow] = useState(null);
  const [target, setTarget] = useState(""), [requests, setRequests] = useState([]);
  const [codes, setCodes] = useState(null), [busy, setBusy] = useState(false);
  const [confirmed, setConfirmed] = useState(false), [cancelled, setCancelled] = useState(false);
  const [message, setMessage] = useState(""), [error, setError] = useState("");
  const end = useRef(null), alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; end.current?.(); }; }, []);
  const run = async operation => {
    if (busy) return;
    setBusy(true); onBusyChange(true); setError("");
    try { await operation(); } catch (failure) { if (alive.current) setError(failure.response?.data?.message || failure.message || "Verification failed."); }
    finally { if (alive.current) { setBusy(false); onBusyChange(false); } }
  };
  const open = () => run(async () => {
    const { withBrowserMatrixVerification } = await import("../lib/matrixVerificationSession.js");
    const started = new Promise((resolve, reject) => {
      withBrowserMatrixVerification(userId, api, value => new Promise(release => {
        end.current = release;
        if (!alive.current) { release(); return; }
        onSessionChange(true); setSession(value); resolve();
      })).catch(reject);
    });
    await started;
  });
  const close = () => { end.current?.(); end.current = null; onSessionChange(false); setSession(null); setFlow(null); setRequests([]); setCodes(null); setConfirmed(false); setCancelled(false); setMessage(""); };
  const start = () => run(async () => {
    await session.refresh();
    const next = await session.createFlow(target.trim());
    setFlow(next); await next.start(); setMessage("Request sent. Accept it on the other device.");
  });
  const accept = request => run(async () => {
    const next = await session.createFlow(request.deviceId);
    setFlow(next); await next.accept(request.flowId); setRequests([]); setMessage("Request accepted. Continue on the requesting device.");
  });
  const action = (name, success) => run(async () => { await flow[name](); setMessage(success); });
  const cancel = () => run(async () => { await flow.cancel(); setCodes(null); setCancelled(true); setMessage("Verification cancelled. Do not trust this device based on these codes."); });
  return <div className="settings-retention" data-verification aria-busy={busy}>
    <h4>Compare device codes</h4>
    <p>Open verification on both devices. The desktop can start a request using the browser device ID shown below. Never confirm unless all three numbers match on both screens.</p>
    {!session ? <button type="button" className="secondary" disabled={busy} onClick={open}>Open verification</button> : <>
      <p>Current device: <code>{session.deviceId}</code> ({session.serverAuthorization}).</p>
      {!flow && <>
        <label className="dialog-field">Other device ID<input value={target} onChange={event => setTarget(event.target.value)} autoComplete="off" /></label>
        <button type="button" className="secondary" disabled={busy || !target.trim()} onClick={start}>Send verification request</button>
        <button type="button" className="secondary" disabled={busy} onClick={() => run(async () => { setRequests(await session.receive()); setMessage("Checked for requests."); })}>Check for requests</button>
        {requests.map(request => <p key={request.flowId}>Request from {request.deviceId} <button type="button" className="secondary" disabled={busy} onClick={() => accept(request)}>Accept request</button></p>)}
      </>}
      {flow && <>
        <p>On the requesting device, start the code exchange. On the other device, accept it. Receive updates and send pending events until codes are ready; on desktop, use Update.</p>
        {flow.hasPendingDelivery && <><p role="alert">A verification event could not be delivered. Retry it before continuing.</p><button type="button" className="secondary" disabled={busy} onClick={() => run(async () => { await flow.retryDelivery(); setMessage("Verification event delivered."); })}>Retry verification delivery</button></>}
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action("receive", "Updates received.")}>Receive updates</button>
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action("startSas", "Code exchange started.")}>Start code exchange</button>
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action("acceptSas", "Code exchange accepted.")}>Accept code exchange</button>
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action("flush", "Pending events sent.")}>Send pending events</button>
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { setCodes(flow.codes()); setMessage("Compare these numbers on both browsers."); })}>Show codes</button>
        {codes && <><p role="status">Security codes: <strong>{codes.join(" · ")}</strong></p><button type="button" className="secondary" disabled={busy || confirmed || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { await flow.confirmCodesMatch(true); setConfirmed(true); setMessage("Confirmed locally. Confirm on the other browser too."); })}>The codes match on both devices</button><button type="button" className="secondary" disabled={busy || confirmed || cancelled || flow.hasPendingDelivery} onClick={cancel}>The codes do not match</button></>}
        {session.serverAuthorization === "authorized" && <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { await flow.approve(); setMessage("Device authorized after verification."); })}>Authorize verified device</button>}
        <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={cancel}>Cancel verification</button>
      </>}
      <button type="button" className="secondary" disabled={busy} onClick={close}>Close verification</button>
    </>}
    {message && <p role="status">{message}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}
