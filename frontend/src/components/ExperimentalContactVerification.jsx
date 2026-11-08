import { useEffect, useRef, useState } from "react";
import { axiosInstance as api } from "../lib/axios";

// Opt-in development interface. The user compares SDK-provided SAS values;
// opening the screen or receiving a server access flag never grants trust.
export default function ExperimentalContactVerification({ accountId, peer }) {
  const [target, setTarget] = useState(""), [flow, setFlow] = useState(null), [requests, setRequests] = useState([]);
  const [codes, setCodes] = useState(null), [confirmed, setConfirmed] = useState(false), [cancelled, setCancelled] = useState(false);
  const [busy, setBusy] = useState(false), [message, setMessage] = useState(""), [error, setError] = useState(""), [interrupted, setInterrupted] = useState(null);
  const alive = useRef(true), end = useRef(null);
  useEffect(() => { alive.current = true; return () => { alive.current = false; end.current?.(); }; }, []);
  const run = async operation => {
    if (busy) return;
    setBusy(true); setError("");
    try { await operation(); } catch (failure) { if (alive.current) { setError(failure.response?.data?.message || failure.message || "Verification failed."); if (failure.name === "InterruptedVerification") setInterrupted(failure.eventId); } }
    finally { if (alive.current) setBusy(false); }
  };
  const close = () => { end.current?.(); end.current = null; setFlow(null); setRequests([]); setCodes(null); setConfirmed(false); setCancelled(false); setInterrupted(null); setMessage(""); };
  const open = () => run(async () => {
    const [{ prepareBrowserDirectMatrixRoom }, { withBrowserMatrixIdentity }, { prepareContactVerification }] = await Promise.all([import("../lib/matrixDeviceClient.js"), import("../lib/matrixIdentityVault.js"), import("../lib/matrixContactVerification.js")]);
    const userId = (await api.get("/matrix/device/identity")).data?.userId;
    if (typeof userId !== "string" || !userId.startsWith(`@${accountId}:`)) throw new Error("Account identity changed.");
    const room = await prepareBrowserDirectMatrixRoom(userId, peer._id, api);
    const peerUser = room.members.find(member => member !== userId);
    await new Promise((resolve, reject) => {
      withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
        const active = await prepareContactVerification(machine, descriptor, room, peerUser, target.trim(), api);
        try { await new Promise(release => { end.current = release; if (!alive.current) { release(); return; } setFlow(active); resolve(); }); }
        finally { active.close(); }
      }).catch(reject);
    });
  });
  const action = (name, notice) => run(async () => { await flow[name](); setMessage(notice); });
  const cancel = () => run(async () => { await flow.cancel(); setCodes(null); setCancelled(true); setMessage("Verification cancelled. Do not trust these codes."); });
  return <section className="settings-retention" data-contact-verification aria-busy={busy}>
    <h3>Experimental contact verification</h3>
    <p>Ask {peer.fullName} for their Matrix device ID through a trusted channel. Both people must open verification, then compare all three numbers on their own screens. This does not migrate messages.</p>
    {!flow ? <><label className="dialog-field">Contact device ID<input value={target} onChange={event => setTarget(event.target.value)} autoComplete="off" spellCheck={false} /></label><button type="button" className="secondary" disabled={busy || !target.trim()} onClick={open}>Open contact verification</button></> : <>
      <p>Contact device: <code>{target}</code></p>
      <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action("start", "Request sent to your contact.")}>Request contact verification</button>
      <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { await flow.receive(); setRequests(flow.incoming()); setMessage("Checked for requests."); })}>Check contact requests</button>
      {requests.map(request => <p key={request.flowId}>Request from {peer.fullName} <button type="button" className="secondary" disabled={busy} onClick={() => run(async () => { await flow.accept(request.flowId); setRequests([]); setMessage("Request accepted."); })}>Accept contact request</button></p>)}
      {flow.hasPendingDelivery && <button type="button" className="secondary" disabled={busy} onClick={() => action("retryDelivery", "Verification event delivered.")}>Retry contact delivery</button>}
      <p>The requester receives updates and starts the exchange. The recipient receives updates and accepts it. Both then receive updates and send pending events until the codes are ready.</p>
      {[["receive", "Receive contact updates"], ["startSas", "Start contact code exchange"], ["acceptSas", "Accept contact code exchange"], ["flush", "Send contact events"]].map(([method, label]) => <button type="button" className="secondary" key={method} disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => action(method, "Verification updated.")}>{label}</button>)}
      <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { setCodes(flow.codes()); setMessage("Compare the numbers on both devices."); })}>Show contact codes</button>
      {codes && <><p role="status">Contact codes: <strong>{codes.join(" · ")}</strong></p><button type="button" className="secondary" disabled={busy || confirmed || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { await flow.confirm(true); setConfirmed(true); setMessage("Confirmed here. Your contact must also confirm."); })}>Contact codes match</button><button type="button" className="secondary" disabled={busy || confirmed || cancelled || flow.hasPendingDelivery} onClick={cancel}>Contact codes do not match</button></>}
      <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={() => run(async () => { setMessage(flow.verified() ? "Contact verification completed." : "Waiting for both confirmations."); })}>Check contact verification</button>
      <button type="button" className="secondary" disabled={busy || cancelled || flow.hasPendingDelivery} onClick={cancel}>Cancel contact verification</button>
      {interrupted && <button type="button" className="secondary" disabled={busy} onClick={() => run(async () => { await flow.abandonInterrupted(interrupted); close(); setMessage("Interrupted verification discarded. Start again on both devices."); })}>Discard interrupted verification</button>}
      <button type="button" className="secondary" disabled={busy} onClick={close}>Close contact verification</button>
    </>}
    {message && <p role="status">{message}</p>}{error && <p role="alert">{error}</p>}
  </section>;
}
