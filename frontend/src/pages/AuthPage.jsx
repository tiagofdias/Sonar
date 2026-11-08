import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import toast from "react-hot-toast";
import {
  ArrowUpRight,
  LockKeyhole,
  ShieldCheck,
  Timer,
  AudioLines,
  KeyRound,
} from "lucide-react";
import { useAuthStore } from "../store/useAuthStore";
import { passkeysSupported } from "../lib/passkeys";
export default function AuthPage({ signup = false, recover = false }) {
  const {
    login,
    loginWithPasskey,
    signup: register,
    isLoggingIn,
    isSigningUp,
  } = useAuthStore();
  const [data, setData] = useState({ fullName: "", username: "", email: "", password: "", recoveryKey: "", confirmation: "" });
  const [recovering, setRecovering] = useState(false);
  const navigate = useNavigate();
  const resetPassword = async () => {
    if (data.password !== data.confirmation) return toast.error("Your new passwords do not match");
    setRecovering(true);
    try {
      await useAuthStore.getState().recover(data.email, data.recoveryKey, data.password);
      toast.success("Password reset. You are signed in.");
      navigate("/", { replace: true });
    } catch (error) {
      toast.error(error.response?.data?.message || error.message);
    } finally {
      setRecovering(false);
    }
  };
  const field = (name, label, type = "text") => (
    <label>
      {label}
      <input
        required
        type={type}
        autoComplete={
          name === "recoveryKey"
            ? "off"
            : name === "password" || name === "confirmation"
            ? signup || recover
              ? "new-password"
              : "current-password"
            : name === "username"
              ? "username"
              : name === "email"
              ? "email"
              : "name"
        }
        minLength={(name === "password" || name === "confirmation") && (signup || recover) ? 10 : undefined}
        value={data[name]}
        onChange={(e) => setData({ ...data, [name]: e.target.value })}
      />
    </label>
  );
  return (
    <main className="auth-page">
      <section className="auth-story">
        <Link to="/" className="wordmark">
          <span className="sonar-mark" />
          sonar<span className="beta">EARLY ACCESS</span>
        </Link>
        <div>
          <div className="eyebrow">LESS NOISE. MORE CONNECTION.</div>
          <h1>
            Your people.
            <br />
            Your conversations.
            <br />
            <em>Your space.</em>
          </h1>
          <p>
            A quieter corner of the internet. Built for the conversations that
            matter, with privacy at the core.
          </p>
          <div className="auth-features">
            <span>
              <LockKeyhole size={17} /> Encrypted messages
            </span>
            <span>
              <Timer size={17} /> Gone in 7 days
            </span>
            <span>
              <AudioLines size={17} /> No call recordings
            </span>
          </div>
        </div>
        <small>Free to use. No subscriptions. No ads.</small>
      </section>
      <section className="auth-form">
        <div className="auth-card">
          <ShieldCheck className="accent" size={30} />
          <h2>{recover ? "Reset your password." : signup ? "Find your frequency." : "Welcome back."}</h2>
          <p>
            {recover
              ? "Use the recovery key you saved when you created your account. Your messages stay encrypted and readable."
              : signup
              ? "Create your own little corner of Sonar."
              : "Good conversations are waiting for you."}
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (recover) resetPassword();
              else (signup ? register : login)(data);
            }}
          >
            {signup && field("fullName", "Display name")}
            {signup && field("username", "Username")}
            {field("email", "Email address", "email")}
            {recover && field("recoveryKey", "Recovery key")}
            {field("password", recover ? "New password" : "Password", "password")}
            {recover && field("confirmation", "Confirm new password", "password")}
            {!signup && !recover && <Link to="/recover" className="forgot-link">Forgot your password?</Link>}
            <button className="primary" disabled={isLoggingIn || isSigningUp || recovering}>
              {recovering
                ? "Resetting your password..."
                : recover
                ? "Reset password"
                : isLoggingIn || isSigningUp
                ? "Setting up your space..."
                : signup
                  ? "Create your account"
                  : "Enter your space"}
              <ArrowUpRight size={18} />
            </button>
          </form>
          {!signup && !recover && (
            <>
              <div className="auth-divider">
                <span>or</span>
              </div>
              <button
                type="button"
                className="secondary passkey-login"
                disabled={isLoggingIn || !passkeysSupported()}
                onClick={loginWithPasskey}
              >
                <KeyRound size={17} />
                Sign in with a passkey
              </button>
              <p className="passkey-login-caption">
                {passkeysSupported()
                  ? "Use a passkey you previously added in account settings."
                  : "Passkeys require a supported browser and HTTPS or localhost."}
              </p>
            </>
          )}
          <p className="auth-switch">
            {signup || recover ? "Remembered it?" : "New around here?"}{" "}
            <Link to={signup || recover ? "/login" : "/signup"}>
              {signup || recover ? "Sign in" : "Join Sonar"}
            </Link>
          </p>
          <div className="device-note">
            <LockKeyhole size={15} />
            <span>
              Your messages are locked with a recovery key that only you
              keep. Sonar never has it, so it can never read your
              conversations, on the web or the desktop app.
            </span>
          </div>
        </div>
      </section>
    </main>
  );
}
