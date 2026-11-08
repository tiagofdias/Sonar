import {
  startAuthentication,
  startRegistration,
  browserSupportsWebAuthn,
} from "@simplewebauthn/browser";
export const passkeysSupported = () =>
  window.isSecureContext && browserSupportsWebAuthn();
export function passkeyError(error) {
  if (error.name === "NotAllowedError" || error.name === "AbortError")
    return "Passkey request cancelled or timed out. You can try again or use your password.";
  if (error.name === "InvalidStateError")
    return "This passkey is already registered. Try a different authenticator.";
  return (
    error.response?.data?.message || error.message || "Passkey request failed"
  );
}
export async function runPasskey(ceremony, options, origin, popup) {
  if (!passkeysSupported())
    throw new Error(
      "Passkeys need a supported browser and HTTPS or localhost.",
    );
  if (origin === window.location.origin)
    return ceremony === "registration"
      ? startRegistration({ optionsJSON: options })
      : startAuthentication({ optionsJSON: options });
  const expected = new URL(origin);
  if (!(
    import.meta.env.DEV &&
    window.location.hostname === "127.0.0.1" &&
    expected.hostname === "localhost" &&
    expected.protocol === "http:" &&
    expected.port === window.location.port
  ))
    throw new Error(
      "Passkey origin does not match this app. Check WEBAUTHN_ORIGIN.",
    );
  if (!popup || popup.closed)
    throw new Error("Allow the localhost passkey window to continue.");
  return new Promise((resolve, reject) => {
    const requestId = crypto.randomUUID();
    const cleanup = () => {
      clearTimeout(timeout);
      clearInterval(closed);
      window.removeEventListener("message", receive);
      popup.close();
    };
    const finish = (error, result) => {
      cleanup();
      if (error) reject(error);
      else resolve(result);
    };
    const receive = (event) => {
      if (event.origin !== origin || event.source !== popup) return;
      if (event.data?.type === "sonar:passkey-ready")
        popup.postMessage(
          { type: "sonar:passkey-request", requestId, ceremony, options },
          origin,
        );
      if (event.data?.requestId !== requestId) return;
      if (event.data?.type === "sonar:passkey-result")
        finish(null, event.data.response);
      if (event.data?.type === "sonar:passkey-error")
        finish(new Error(event.data.message));
    };
    const timeout = setTimeout(
      () => finish(new Error("Passkey request timed out. Try again.")),
      180000,
    );
    const closed = setInterval(() => {
      if (popup.closed) finish(new Error("Passkey window was closed."));
    }, 500);
    window.addEventListener("message", receive);
    popup.location.href = `${origin}/passkey`;
  });
}
export function openPasskeyWindow() {
  if (import.meta.env.DEV && window.location.hostname === "127.0.0.1") {
    const popup = window.open(
      "about:blank",
      "sonar-passkey",
      "popup,width=480,height=560",
    );
    if (!popup)
      throw new Error(
        "Allow popups for Sonar to use the localhost passkey prompt.",
      );
    return popup;
  }
  return null;
}
export async function passkeyRequest(path, body = {}) {
  const response = await fetch(`/api/auth/passkeys/${path}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const result = await response.json();
  if (!response.ok) throw new Error(result.message || "Passkey request failed");
  return result;
}
