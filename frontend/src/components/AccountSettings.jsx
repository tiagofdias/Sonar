import { useEffect, useRef, useState } from "react";
import {
  Check,
  Copy,
  KeyRound,
  LockKeyhole,
  LogOut,
  ShieldCheck,
  UserRound,
  X,
  Headphones,
} from "lucide-react";
import toast from "react-hot-toast";
import { useAuthStore } from "../store/useAuthStore";
import { axiosInstance as api } from "../lib/axios";
import { fingerprint } from "../lib/crypto";
import { rewrapBackup } from "../lib/keyBackup";
import { backupKind } from "../lib/legacyIdentity";
import PasskeySettings from "./PasskeySettings";
import VoiceSettings from "./VoiceSettings";
import SessionSettings from "./SessionSettings";
import ExperimentalDeviceSettings from "./ExperimentalDeviceSettings";
import { useUnreadStore } from "../store/useUnreadStore";

export default function AccountSettings() {
  const { authUser, updateProfile, logout, isUpdatingProfile } = useAuthStore();
  const [tab, setTab] = useState("profile"),
    [name, setName] = useState(authUser.fullName),
    [username, setUsername] = useState(authUser.username || ""),
    [passwords, setPasswords] = useState({
      currentPassword: "",
      newPassword: "",
      confirmation: "",
    }),
    [showPasswords, setShowPasswords] = useState(false),
    [changing, setChanging] = useState(false),
    [keyFingerprint, setKeyFingerprint] = useState(""),
    [backedUp, setBackedUp] = useState(null),
    [syncPassword, setSyncPassword] = useState(""),
    [syncing, setSyncing] = useState(false);
  const notificationsEnabled = useUnreadStore(state => state.notificationsEnabled);
  const dialog = useRef(null);
  const [passkeyBusy, setPasskeyBusy] = useState(false);
  const [deviceBusy, setDeviceBusy] = useState(false);
  const busy = changing || isUpdatingProfile || passkeyBusy || deviceBusy || syncing;
  useEffect(() => {
    let active = true;
    backupKind(authUser, api).then((value) => { if (active) setBackedUp(value || false); }).catch(() => { if (active) setBackedUp(null); });
    return () => { active = false; };
  }, [authUser]);
  const enableSync = async (event) => {
    event.preventDefault();
    if (!syncPassword || busy) return;
    setSyncing(true);
    try {
      if (backedUp) await useAuthStore.getState().newRecoveryKeyFor(syncPassword);
      else await useAuthStore.getState().rotateIdentity(syncPassword);
      setSyncPassword("");
      setBackedUp("recovery");
    } catch (error) {
      toast.error(error.response?.data?.message || error.message);
    } finally {
      setSyncing(false);
    }
  };
  const close = () => {
    if (!busy) useAuthStore.setState({ settingsOpen: false });
  };
  useEffect(() => {
    let active = true;
    if (authUser.encryptionKey)
      fingerprint(authUser.encryptionKey)
        .then((value) => {
          if (active) setKeyFingerprint(value);
        })
        .catch(() => {
          if (active) setKeyFingerprint("Safety code unavailable");
        });
    return () => {
      active = false;
    };
  }, [authUser.encryptionKey]);
  useEffect(() => {
    const previous = document.activeElement;
    dialog.current?.querySelector("button")?.focus();
    return () => previous?.focus?.();
  }, []);
  useEffect(() => {
    const keydown = (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
      if (event.key !== "Tab") return;
      const elements = [
        ...dialog.current.querySelectorAll(
          'button:not(:disabled), input:not(:disabled), [tabindex="0"]',
        ),
      ].filter((element) => element.offsetParent !== null);
      const first = elements[0],
        last = elements.at(-1);
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !dialog.current.contains(document.activeElement))
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", keydown);
    return () => document.removeEventListener("keydown", keydown);
  }, [busy]);
  const handle = username.trim().replace(/^@/, "").toLowerCase();
  const saveName = async (event) => {
    event.preventDefault();
    if (name.trim() && !busy) await updateProfile({ fullName: name.trim(), username: handle });
  };
  const changePassword = async (event) => {
    event.preventDefault();
    if (changing) return;
    if (passwords.newPassword !== passwords.confirmation) {
      toast.error("Your new passwords do not match");
      return;
    }
    if (passwords.currentPassword === passwords.newPassword) {
      toast.error("Choose a different password");
      return;
    }
    if (new TextEncoder().encode(passwords.newPassword).length > 72) {
      toast.error("Password must be at most 72 UTF-8 bytes");
      return;
    }
    setChanging(true);
    try {
      // Re-protect the encrypted message key with the new password, otherwise
      // other devices could no longer unlock it.
      const { backup } = (await api.get("/auth/key-backup")).data || {};
      // Only legacy password-protected backups depend on the password.
      const keyBackup = backup?.version === 1
        ? await rewrapBackup(backup, passwords.currentPassword, passwords.newPassword, authUser._id)
        : undefined;
      await api.put("/auth/password", {
        currentPassword: passwords.currentPassword,
        newPassword: passwords.newPassword,
        keyBackup,
      });
      setPasswords({ currentPassword: "", newPassword: "", confirmation: "" });
      useAuthStore.getState().clearSession();
      toast.success("Password updated. Sign in with your new password.", {
        duration: 6000,
      });
    } catch (error) {
      toast.error(error.response?.data?.message || (error.name === "WrongKeyPassword" ? "Current password is incorrect" : "Could not change password"));
    } finally {
      setChanging(false);
    }
  };
  const switchTab = (value) => {
    setPasswords({ currentPassword: "", newPassword: "", confirmation: "" });
    setShowPasswords(false);
    setTab(value);
  };
  return (
    <div className="modal-backdrop settings-backdrop" onClick={close}>
      <section
        className="account-settings"
        role="dialog"
        aria-modal="true"
        aria-labelledby="account-settings-title"
        ref={dialog}
        onClick={(event) => event.stopPropagation()}
      >
        <aside className="settings-navigation">
          <span className="eyebrow">YOUR SONAR</span>
          <h2 id="account-settings-title">Make it yours.</h2>
          <div className="settings-avatar">
            {authUser.fullName
              .trim()
              .split(/\s+/)
              .map((part) => part[0])
              .slice(0, 2)
              .join("")
              .toUpperCase()}
          </div>
          <strong className="settings-user-name">{authUser.fullName}</strong>
          {authUser.username && <span className="settings-user-handle">@{authUser.username}</span>}
          <span className="settings-user-email">{authUser.email}</span>
          <nav aria-label="Account settings">
            {[
              ["profile", UserRound, "Your profile"],
              ["security", KeyRound, "Password & passkeys"],
              ["privacy", ShieldCheck, "Privacy & device"],
              ["voice", Headphones, "Voice & audio"],
            ].map(([value, Icon, label]) => (
              <button
                key={value}
                className={`nav-row ${tab === value ? "selected" : ""}`}
                disabled={busy}
                aria-current={tab === value ? "page" : undefined}
                onClick={() => switchTab(value)}
              >
                <Icon size={17} />
                {label}
              </button>
            ))}
          </nav>
          <button
            className="nav-row settings-signout"
            disabled={busy}
            onClick={logout}
          >
            <LogOut size={16} />
            Sign out everywhere
          </button>
          <small>Free to use. Your space to keep.</small>
        </aside>
        <div className="settings-content">
          <button
            className="modal-close icon-button"
            aria-label="Close settings"
            onClick={close}
            disabled={busy}
          >
            <X size={20} />
          </button>
          {tab === "profile" ? (
            <>
              <span className="eyebrow">A FAMILIAR FACE</span>
              <h3>Your profile</h3>
              <p>The name your people see in conversations and communities.</p>
              <form onSubmit={saveName}>
                <label className="dialog-field">
                  Display name
                  <input
                    autoComplete="name"
                    required
                    maxLength={80}
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                  />
                </label>
                <label className="dialog-field">
                  Username
                  <input
                    autoComplete="username"
                    required
                    maxLength={25}
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                  />
                  <small>People can find you with this. Letters, numbers, dots and underscores.</small>
                </label>
                <label className="dialog-field">
                  Email address
                  <input type="email" readOnly value={authUser.email} />
                  <small>Email changes are not available yet.</small>
                </label>
                <button
                  className="primary"
                  disabled={
                    busy || !name.trim() || !handle || (name.trim() === authUser.fullName && handle === authUser.username)
                  }
                >
                  {isUpdatingProfile ? "Saving..." : "Save changes"}
                  <Check size={16} />
                </button>
              </form>
              <div className="honest-note">
                <UserRound size={16} />
                <p>
                  Your display name and email are account information visible to
                  the service. Message content is encrypted separately on your
                  device.
                </p>
              </div>
            </>
          ) : tab === "security" ? (
            <>
              <span className="eyebrow">KEEP YOUR SPACE YOURS</span>
              <h3>Password & passkeys</h3>
              <p>Choose how you sign in and keep your account secure.</p>
              <PasskeySettings onBusyChange={setPasskeyBusy} />
              <SessionSettings />
              <section className="security-card password-settings">
                <div className="security-card-heading">
                  <span className="security-icon">
                    <LockKeyhole size={20} />
                  </span>
                  <div>
                    <h4>Password</h4>
                    <p>Updating it signs you out on all devices.</p>
                  </div>
                </div>
                <form className="password-form" onSubmit={changePassword}>
                  {[
                    ["currentPassword", "Current password", "current-password"],
                    ["newPassword", "New password", "new-password"],
                    ["confirmation", "Confirm new password", "new-password"],
                  ].map(([field, label, autoComplete]) => (
                    <label
                      className={`dialog-field ${field === "currentPassword" ? "full-width" : ""}`}
                      key={field}
                    >
                      {label}
                      <input
                        required
                        type={showPasswords ? "text" : "password"}
                        autoComplete={autoComplete}
                        minLength={field === "currentPassword" ? undefined : 10}
                        maxLength={72}
                        value={passwords[field]}
                        onChange={(event) =>
                          setPasswords((current) => ({
                            ...current,
                            [field]: event.target.value,
                          }))
                        }
                      />
                    </label>
                  ))}
                  <label className="settings-checkbox">
                    <input
                      type="checkbox"
                      checked={showPasswords}
                      onChange={(event) =>
                        setShowPasswords(event.target.checked)
                      }
                    />
                    Show passwords
                  </label>
                  <p className="settings-caption">
                    Use at least 10 characters.
                  </p>
                  <button className="primary" disabled={busy}>
                    {changing
                      ? "Updating password..."
                      : "Update password & sign out"}
                    <KeyRound size={16} />
                  </button>
                </form>
                <div className="password-preservation-note">
                  <LockKeyhole size={16} />
                  <p>
                    Your message key is re-protected with the new password on
                    this device. Your encrypted history is kept.
                  </p>
                </div>
              </section>
            </>
          ) : tab === "voice" ? (
            <VoiceSettings />
          ) : (
            <>
              <span className="eyebrow">PRIVACY IN PLAIN SIGHT</span>
              <h3>Your message key</h3>
              <p>
                Messages are encrypted with a key that only your devices hold.
                It is locked with your recovery key before it is stored, so the
                web app and the desktop app can both open your conversations.
              </p>
              <div className="settings-device">
                <ShieldCheck size={21} />
                <div>
                  <strong>{backedUp === "recovery" ? "Protected by your recovery key" : backedUp === "password" ? "Protected by your password" : backedUp === false ? "Only on this browser" : "Checking your key..."}</strong>
                  <small>{backedUp === "recovery" ? "Only your recovery key can unlock it · AES-256-GCM" : backedUp === "password" ? "Set up a recovery key so the server can never unlock it" : backedUp === false ? "Other devices cannot open your messages yet" : " "}</small>
                </div>
                <span className={backedUp === "recovery" ? "green-dot" : "offline-dot"} />
              </div>
              {backedUp !== null && (
                <form className="sync-form" onSubmit={enableSync}>
                  <strong>{backedUp === false ? "Turn on multi-device access" : backedUp === "password" ? "Set up a recovery key" : "Create a new recovery key"}</strong>
                  <p>{backedUp === false
                    ? "Creates a new key protected by a recovery key. Messages you already have stay readable in this browser. Your contacts will be asked to confirm your new key."
                    : "Your recovery key unlocks your messages on a new device and resets a forgotten password. A new one replaces the old one."}</p>
                  <label className="dialog-field">
                    Current password
                    <input type="password" required autoComplete="current-password" value={syncPassword} onChange={(event) => setSyncPassword(event.target.value)} />
                  </label>
                  <button className="primary" disabled={busy || !syncPassword}>
                    {syncing ? "Securing your key..." : backedUp === false ? "Turn on multi-device access" : "Create recovery key"}
                    <KeyRound size={16} />
                  </button>
                </form>
              )}
              <div className="fingerprint">
                <strong>Your safety code</strong>
                <code>{keyFingerprint || "Loading safety code..."}</code>
                <button
                  className="secondary"
                  disabled={
                    !keyFingerprint ||
                    keyFingerprint === "Safety code unavailable"
                  }
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(keyFingerprint);
                      toast.success("Safety code copied");
                    } catch {
                      toast.error("Select the safety code to copy it manually");
                    }
                  }}
                >
                  <Copy size={14} />
                  Copy safety code
                </button>
              </div>
              <p className="settings-caption">
                Read this code to a contact in person or on a call so they can confirm it is really
                you. It is a code, not a fingerprint scan.
              </p>
              {import.meta.env.VITE_SONAR_MATRIX_EXPERIMENTAL === "true" && <ExperimentalDeviceSettings accountId={authUser._id} onBusyChange={setDeviceBusy} />}
              <div className="settings-retention">
                <h4>Private notifications</h4>
                <p>While Sonar is open in the background, show a generic alert for new direct messages. Alerts never include the sender or message text. This setting resets when you sign out or reload.</p>
                <button className="secondary" onClick={async () => {
                  if (notificationsEnabled) { useUnreadStore.getState().setNotificationsEnabled(false); return; }
                  if (!("Notification" in window)) { toast.error("This browser does not support notifications."); return; }
                  try {
                    const permission = await Notification.requestPermission();
                    if (permission === "granted") useUnreadStore.getState().setNotificationsEnabled(true);
                    else toast.error("Allow notifications in your browser to enable alerts.");
                  } catch { toast.error("Could not enable notifications in this browser."); }
                }}>{notificationsEnabled ? "Turn off notifications" : "Enable notifications"}</button>
              </div>
              <div className="settings-retention">
                <h4>A short memory, by design.</h4>
                <p>
                  Messages expire after seven days unless pinned. Calls have no
                  recording or storage in Sonar.
                </p>
              </div>
              <div className="honest-note">
                <LockKeyhole size={16} />
                <p>
                  If you lose your recovery key and every signed-in device, your
                  encrypted history cannot be recovered. Forward secrecy is not implemented. Metadata
                  remains visible to the service.
                </p>
              </div>
            </>
          )}
        </div>
      </section>
    </div>
  );
}
