import { useEffect, useState } from "react";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";
import { useAuthStore } from "../store/useAuthStore";
import {
  openPasskeyWindow,
  runPasskey,
  passkeyRequest,
  passkeyError,
  passkeysSupported,
} from "../lib/passkeys";
export default function PasskeySettings({ onBusyChange }) {
  const [keys, setKeys] = useState([]),
    [loading, setLoading] = useState(true),
    [pending, setPending] = useState(false),
    [name, setName] = useState("My passkey"),
    [password, setPassword] = useState(""),
    [removing, setRemoving] = useState(null),
    [adding, setAdding] = useState(false);
  const load = async () => {
    try {
      setKeys((await api.get("/auth/passkeys")).data);
    } catch (error) {
      toast.error(passkeyError(error));
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    load();
  }, []);
  const busy = (value) => {
    setPending(value);
    onBusyChange(value);
  };
  const add = async (event) => {
    event.preventDefault();
    if (pending) return;
    let popup;
    busy(true);
    try {
      popup = openPasskeyWindow();
      const { options, origin } = await passkeyRequest("register/options", {
        name,
        currentPassword: password,
      });
      setPassword("");
      const response = await runPasskey("registration", options, origin, popup);
      await passkeyRequest("register/verify", { response });
      await load();
      setAdding(false);
      toast.success("Passkey added. You can now use it to sign in.");
    } catch (error) {
      toast.error(passkeyError(error));
    } finally {
      popup?.close();
      busy(false);
    }
  };
  const remove = async () => {
    if (pending || !removing) return;
    if (!password) {
      toast.error("Enter your current password to remove a passkey");
      return;
    }
    busy(true);
    try {
      await api.delete(`/auth/passkeys/${removing._id}`, {
        data: { currentPassword: password },
      });
      setPassword("");
      useAuthStore.getState().clearSession();
      toast.success("Passkey removed. Sign in again.");
    } catch (error) {
      toast.error(passkeyError(error));
    } finally {
      busy(false);
    }
  };
  return (
    <section
      className="security-card passkey-settings"
      aria-labelledby="passkey-settings-heading"
    >
      <div className="security-card-heading">
        <span className="security-icon">
          <KeyRound size={20} />
        </span>
        <div>
          <h4 id="passkey-settings-heading">
            Passkeys <span className="recommended-badge">RECOMMENDED</span>
          </h4>
          <p>A faster sign-in with your face, fingerprint, or device PIN.</p>
        </div>
      </div>
      {loading ? (
        <p className="settings-caption">Loading your passkeys...</p>
      ) : keys.length ? (
        <ul className="passkey-list">
          {keys.map((key) => (
            <li key={key._id}>
              <KeyRound size={16} />
              <div>
                <strong>{key.name}</strong>
                <small>
                  {key.lastUsedAt
                    ? `Last used ${new Date(key.lastUsedAt).toLocaleDateString()}`
                    : `Added ${new Date(key.createdAt).toLocaleDateString()}`}
                </small>
              </div>
              <button
                className="icon-button"
                aria-label={`Remove ${key.name}`}
                disabled={pending}
                onClick={() => {
                  setRemoving(key);
                  setAdding(false);
                  setPassword("");
                }}
              >
                <Trash2 size={15} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <div className="passkey-empty">
          <KeyRound size={21} />
          <div>
            <strong>No passkeys added yet</strong>
            <p>Add one to sign in without typing your password.</p>
          </div>
        </div>
      )}
      {!adding && !removing && (
        <button
          className="secondary"
          disabled={
            pending || loading || !passkeysSupported() || keys.length >= 10
          }
          onClick={() => {
            setAdding(true);
            setPassword("");
          }}
        >
          <Plus size={15} />
          Add a passkey
        </button>
      )}
      {adding && (
        <form className="passkey-setup" onSubmit={add}>
          <h5>Set up a passkey</h5>
          <label className="dialog-field">
            Give it a name
            <input
              required
              maxLength={60}
              value={name}
              disabled={pending}
              onChange={(event) => setName(event.target.value)}
              autoComplete="off"
              placeholder="e.g. My laptop"
            />
          </label>
          <label className="dialog-field">
            Current password
            <input
              required
              type="password"
              autoComplete="current-password"
              value={password}
              disabled={pending}
              onChange={(event) => setPassword(event.target.value)}
            />
            <small>Confirm it is you before adding a new sign-in method.</small>
          </label>
          <div className="security-form-actions">
            <button
              className="primary"
              disabled={pending || !passkeysSupported()}
            >
              <KeyRound size={15} />
              {pending ? "Waiting for your device..." : "Create passkey"}
            </button>
            <button
              className="text-button"
              type="button"
              disabled={pending}
              onClick={() => {
                setAdding(false);
                setPassword("");
              }}
            >
              Cancel
            </button>
          </div>
        </form>
      )}
      {removing && (
        <form
          className="passkey-remove-confirm"
          onSubmit={(event) => {
            event.preventDefault();
            remove();
          }}
        >
          <strong>Remove {removing.name}?</strong>
          <p>
            This signs out all sessions. You can still sign in with your
            password.
          </p>
          <label className="dialog-field">
            Current password
            <input
              required
              type="password"
              autoComplete="current-password"
              value={password}
              disabled={pending}
              onChange={(event) => setPassword(event.target.value)}
            />
          </label>
          <div className="security-form-actions">
            <button className="danger-button" disabled={pending}>
              Remove & sign out
            </button>
            <button
              type="button"
              className="text-button"
              disabled={pending}
              onClick={() => {
                setRemoving(null);
                setPassword("");
              }}
            >
              Cancel
            </button>
          </div>
        </form>
      )}
      {!passkeysSupported() && (
        <p className="settings-caption">
          Passkeys need a supported browser and HTTPS or localhost.
        </p>
      )}
      <p className="passkey-device-note">
        Your passkey signs you in. Your messages unlock with your recovery key
        the first time you use a new device.
      </p>
    </section>
  );
}
