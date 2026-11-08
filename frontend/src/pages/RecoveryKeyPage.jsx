import { useState } from "react";
import toast from "react-hot-toast";
import { ArrowUpRight, Copy, Download, KeyRound, LockKeyhole } from "lucide-react";
import { useAuthStore } from "../store/useAuthStore";

// Shown once whenever a new recovery key is created. The key never leaves this
// device unencrypted and Sonar cannot show it again.
export default function RecoveryKeyPage({ recoveryKey }) {
  const dismiss = useAuthStore((state) => state.dismissRecoveryKey);
  const [saved, setSaved] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(recoveryKey);
      toast.success("Recovery key copied");
    } catch {
      toast.error("Select the key to copy it manually");
    }
  };
  const download = () => {
    const blob = new Blob([`Sonar recovery key\n\n${recoveryKey}\n\nUse it to open your messages on a new device and to reset a forgotten password.\nKeep it private. Anyone with this key and your email can reset your password.\n`], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const link = Object.assign(document.createElement("a"), { href: url, download: "sonar-recovery-key.txt" });
    link.click();
    URL.revokeObjectURL(url);
  };
  return (
    <main className="auth-page unlock-page">
      <section className="auth-form">
        <div className="auth-card">
          <KeyRound className="accent" size={30} />
          <h2>Save your recovery key.</h2>
          <p>
            It unlocks your messages on a new device and resets your password if you forget it.
            Only you have it: Sonar cannot show it again or recover it for you.
          </p>
          <div className="recovery-key" aria-label="Recovery key">{recoveryKey}</div>
          <div className="recovery-actions">
            <button type="button" className="secondary" onClick={copy}><Copy size={15} /> Copy</button>
            <button type="button" className="secondary" onClick={download}><Download size={15} /> Save as file</button>
          </div>
          <label className="settings-checkbox recovery-confirm">
            <input type="checkbox" checked={saved} onChange={(event) => setSaved(event.target.checked)} />
            I saved my recovery key somewhere safe
          </label>
          <button type="button" className="primary" disabled={!saved} onClick={dismiss}>
            Continue to Sonar <ArrowUpRight size={18} />
          </button>
          <div className="device-note">
            <LockKeyhole size={15} />
            <span>Store it in a password manager or on paper. Anyone with this key and your email address can reset your password.</span>
          </div>
        </div>
      </section>
    </main>
  );
}
