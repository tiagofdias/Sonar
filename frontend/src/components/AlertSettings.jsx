import { useEffect, useState } from "react";
import toast from "react-hot-toast";
import { Ban, Bell, BellRing, MessageCircle, Search } from "lucide-react";
import { usePreferencesStore } from "../store/usePreferencesStore";
import { useUnreadStore } from "../store/useUnreadStore";
import { disablePush, enablePush, pushEnabled, pushSupported } from "../lib/pushAlerts";
import Avatar from "./Avatar";

// One card per topic, the same cards (and words) as the desktop app's Alerts & privacy.
function Card({ icon: Icon, title, note, children }) {
  return (
    <section className="security-card">
      <div className="security-card-heading">
        <span className="security-icon"><Icon size={19} /></span>
        <div><h4>{title}</h4><p>{note}</p></div>
      </div>
      {children}
    </section>
  );
}

export default function AlertSettings() {
  const preferences = usePreferencesStore();
  const notificationsEnabled = useUnreadStore((state) => state.notificationsEnabled);
  const [word, setWord] = useState(""), [push, setPush] = useState(false), [pushBusy, setPushBusy] = useState(false);
  useEffect(() => { preferences.load(); pushEnabled().then(setPush); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const addWord = (event) => {
    event.preventDefault();
    const value = word.trim();
    if (value && !preferences.keywords.includes(value) && preferences.keywords.length < 30) preferences.setDevice({ keywords: [...preferences.keywords, value] });
    setWord("");
  };
  const togglePush = async () => {
    setPushBusy(true);
    try {
      if (push) await disablePush(); else await enablePush();
      setPush(!push);
    } catch (error) { toast.error(error.response?.data?.message || error.message); }
    finally { setPushBusy(false); }
  };
  const toggleNotifications = async () => {
    if (notificationsEnabled) { useUnreadStore.getState().setNotificationsEnabled(false); return; }
    if (!("Notification" in window)) { toast.error("This browser does not support notifications."); return; }
    try {
      const permission = await Notification.requestPermission();
      if (permission === "granted") useUnreadStore.getState().setNotificationsEnabled(true);
      else toast.error("Allow notifications in your browser to enable alerts.");
    } catch { toast.error("Could not enable notifications in this browser."); }
  };
  return (
    <>
      <span className="eyebrow">ONLY WHAT MATTERS</span>
      <h3>Alerts &amp; privacy</h3>
      <p>Choose what gets your attention, and who can reach you.</p>

      <Card icon={Bell} title="Sounds" note="Message sounds and the ringtone. Separate from call volume.">
        <div className="settings-slider">
          <label className="output-volume-label" htmlFor="alert-volume">Alert volume <output htmlFor="alert-volume">{Math.round(preferences.alertVolume * 100)}%</output></label>
          <input id="alert-volume" aria-label="Alert volume" type="range" min="0" max="1" step="0.05" value={preferences.alertVolume} onChange={(event) => preferences.setDevice({ alertVolume: Number(event.target.value) })} />
        </div>
      </Card>

      <Card icon={MessageCircle} title="Calls & typing" note="Small things that change how conversations feel.">
        <div className="settings-checks">
          <label className="settings-checkbox">
            <input type="checkbox" checked={preferences.confirmCalls} onChange={(event) => preferences.setDevice({ confirmCalls: event.target.checked })} />
            Ask before starting a call
          </label>
          <label className="settings-checkbox">
            <input type="checkbox" checked={preferences.typingHidden} onChange={(event) => preferences.save({ typingHidden: event.target.checked })} />
            Turn off typing indicators (yours and everyone else&apos;s)
          </label>
        </div>
      </Card>

      <Card icon={BellRing} title="Alerts on this device" note="Alerts never include the sender or the message text.">
        <div className="settings-row">
          <span><strong>While Sonar is in the background</strong><small>A generic alert for new direct messages. Resets when you sign out or reload.</small></span>
          <button className="secondary compact" onClick={toggleNotifications}>{notificationsEnabled ? "Turn off" : "Turn on"}</button>
        </div>
        {pushSupported() && (
          <div className="settings-row">
            <span><strong>When Sonar is closed</strong><small>Your browser says something arrived.</small></span>
            <button className="secondary compact" disabled={pushBusy} onClick={togglePush}>{push ? "Turn off" : "Turn on"}</button>
          </div>
        )}
      </Card>

      <Card icon={Search} title="Keyword alerts" note="Alert me when these words appear, even in muted servers. Matched on this device, so the server never sees them.">
        <form className="settings-inline-form" onSubmit={addWord}>
          <input aria-label="Keyword" maxLength={40} placeholder="Add a word" value={word} onChange={(event) => setWord(event.target.value)} />
          <button className="primary compact" disabled={!word.trim()}>Add</button>
        </form>
        {preferences.keywords.length > 0 && (
          <div className="keyword-list">
            {preferences.keywords.map((item) => <button key={item} className="secondary compact" title="Remove" onClick={() => preferences.setDevice({ keywords: preferences.keywords.filter((other) => other !== item) })}>{item} ×</button>)}
          </div>
        )}
      </Card>

      <Card icon={Ban} title="Blocked people" note="They cannot message, call or send you requests, and their server messages are hidden completely. They are not told.">
        {!preferences.blocked.length && <p className="settings-caption">Nobody is blocked.</p>}
        {preferences.blocked.map((person) => (
          <div className="person friend-request" key={person._id}>
            <Avatar user={person} />
            <span><strong>{person.fullName}</strong>{person.username && <small>@{person.username}</small>}</span>
            <button className="secondary compact" onClick={() => preferences.unblock(person._id)}>Unblock</button>
          </div>
        ))}
      </Card>
    </>
  );
}
