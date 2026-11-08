import { useEffect, useState } from "react";
import { KeyRound } from "lucide-react";
import {
  startAuthentication,
  startRegistration,
} from "@simplewebauthn/browser";
import { passkeyError } from "../lib/passkeys";
export default function PasskeyWindow() {
  const [request, setRequest] = useState(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const parentOrigin = `http://127.0.0.1:${window.location.port}`;
  useEffect(() => {
    if (
      !import.meta.env.DEV ||
      window.location.hostname !== "localhost" ||
      !window.opener
    )
      return;
    const receive = (event) => {
      if (
        event.origin !== parentOrigin ||
        event.source !== window.opener ||
        event.data?.type !== "sonar:passkey-request"
      )
        return;
      const { options, ceremony } = event.data;
      if (
        !["registration", "authentication"].includes(ceremony) ||
        (options?.rp?.id || options?.rpId) !== "localhost"
      )
        return;
      setRequest((current) => current || event.data);
    };
    window.addEventListener("message", receive);
    window.opener.postMessage({ type: "sonar:passkey-ready" }, parentOrigin);
    return () => window.removeEventListener("message", receive);
  }, []);
  const proceed = async () => {
    if (!request || busy) return;
    setBusy(true);
    setError("");
    try {
      const response =
        request.ceremony === "registration"
          ? await startRegistration({ optionsJSON: request.options })
          : await startAuthentication({ optionsJSON: request.options });
      window.opener.postMessage(
        {
          type: "sonar:passkey-result",
          requestId: request.requestId,
          response,
        },
        parentOrigin,
      );
    } catch (error) {
      const message = passkeyError(error);
      setError(message);
      window.opener?.postMessage(
        { type: "sonar:passkey-error", requestId: request.requestId, message },
        parentOrigin,
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <main className="passkey-window">
      <KeyRound size={38} className="accent" />
      <span className="eyebrow">SONAR · LOCAL PASSKEY PROMPT</span>
      <h1>
        {request?.ceremony === "registration"
          ? "Create your passkey."
          : "Welcome back."}
      </h1>
      <p>
        Your browser will ask for your fingerprint, face, device PIN, or
        security key. Your chat encryption keys stay in the original Sonar
        window.
      </p>
      <button className="primary" disabled={!request || busy} onClick={proceed}>
        {busy
          ? "Waiting for your device..."
          : request
            ? "Continue with passkey"
            : "Waiting for Sonar..."}
      </button>
      {error && <p role="alert">{error}</p>}
      <small>Return to your original Sonar window when finished.</small>
    </main>
  );
}
