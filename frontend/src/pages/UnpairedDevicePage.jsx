import { useState } from "react";
import toast from "react-hot-toast";
import { ArrowUpRight, KeyRound, LockKeyhole, LogOut, RotateCcw } from "lucide-react";
import { useAuthStore } from "../store/useAuthStore";

// Shown when the account is signed in but this device does not hold its
// encryption key yet:
//   recovery - unlock the backup with the recovery key
//   locked   - unlock a backup made before recovery keys, with the password
//   missing  - the key exists only on an older device without a backup
// In any case the user can reset the key (older messages become unreadable).
export default function UnpairedDevicePage({ user, reason }) {
  const { unlockIdentity, rotateIdentity, logout } = useAuthStore();
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [resetting, setResetting] = useState(reason === "missing");
  const [confirmReset, setConfirmReset] = useState(false);
  const run = async (action, success) => {
    if (!secret || busy) return;
    setBusy(true);
    try {
      if (await action(secret)) toast.success(success);
    } catch (error) {
      toast.error(error.response?.data?.message || error.message);
    } finally {
      setBusy(false);
    }
  };
  const submit = (event) => {
    event.preventDefault();
    if (!resetting) return run(unlockIdentity, "Your messages are unlocked on this device");
    if (!confirmReset) return setConfirmReset(true);
    return run(rotateIdentity, "New encryption key created");
  };
  const recovery = reason === "recovery" && !resetting;
  return (
    <main className="auth-page unlock-page">
      <section className="auth-form">
        <div className="auth-card">
          <LockKeyhole className="accent" size={30} />
          <h2>{resetting ? (reason === "missing" ? "Your key lives elsewhere." : "Reset your encryption key.") : "Unlock your messages."}</h2>
          <p>
            {resetting
              ? reason === "missing"
                ? `Signed in as ${user.email}. This account's message key is only on the browser where you first used Sonar.`
                : `Signed in as ${user.email}. Without your recovery key, Sonar can create a new message key. Messages encrypted for the old key cannot be opened with it.`
              : recovery
                ? `Signed in as ${user.email}. Enter the recovery key you saved when you created your account. It unlocks your messages on this device; Sonar never sees it.`
                : `Signed in as ${user.email}. Your password decrypts your message key on this device. You will then get a recovery key.`}
          </p>
          {reason === "missing" && (
            <div className="unlock-steps">
              <strong>To keep your history</strong>
              <span>Open Sonar on that browser, then go to Account settings → Privacy &amp; device → Turn on multi-device access. Then sign in here again.</span>
            </div>
          )}
          <form onSubmit={submit}>
            <label>
              {recovery ? "Recovery key" : "Password"}
              <input
                type={recovery ? "text" : "password"}
                required
                autoComplete={recovery ? "off" : "current-password"}
                spellCheck={false}
                placeholder={recovery ? "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX" : undefined}
                value={secret}
                onChange={(event) => setSecret(event.target.value)}
              />
            </label>
            {resetting && confirmReset && (
              <p className="unlock-warning">
                A new key replaces the old one. Messages encrypted for the old key stay unreadable here, and your contacts will be asked to confirm your new safety code.
              </p>
            )}
            <button className="primary" disabled={busy || !secret}>
              {busy ? "Working on it..." : !resetting ? "Unlock messages" : confirmReset ? "Yes, create a new key" : "Reset my encryption key"}
              {!resetting ? <KeyRound size={18} /> : confirmReset ? <RotateCcw size={18} /> : <ArrowUpRight size={18} />}
            </button>
          </form>
          {reason !== "missing" && (
            <button type="button" className="text-button" disabled={busy} onClick={() => { setResetting(!resetting); setConfirmReset(false); setSecret(""); }}>
              {resetting ? "Back to unlocking" : recovery ? "Lost your recovery key?" : "Forgot your password?"}
            </button>
          )}
          <button type="button" className="secondary passkey-login" disabled={busy} onClick={logout}>
            <LogOut size={17} /> Sign out
          </button>
          <div className="device-note">
            <LockKeyhole size={15} />
            <span>Your message key is locked with your recovery key before it is stored. If you lose the recovery key and every signed-in device, encrypted history cannot be recovered.</span>
          </div>
        </div>
      </section>
    </main>
  );
}
