import { useEffect, useState } from "react";
import toast from "react-hot-toast";
import { usePreferencesStore } from "../store/usePreferencesStore";
import { disablePush, enablePush, pushEnabled, pushSupported } from "../lib/pushAlerts";
import Avatar from "./Avatar";

export default function AlertSettings() {
  const preferences = usePreferencesStore();
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
  return (
    <>
      <span className="eyebrow">ONLY WHAT MATTERS</span>
      <h3>Alerts &amp; privacy</h3>
      <p>Choose what gets your attention, and who can reach you.</p>
      <div className="settings-retention">
        <label className="output-volume-label" htmlFor="alert-volume">Alert volume <output htmlFor="alert-volume">{Math.round(preferences.alertVolume * 100)}%</output></label>
        <input id="alert-volume" aria-label="Alert volume" type="range" min="0" max="1" step="0.05" value={preferences.alertVolume} onChange={(event) => preferences.setDevice({ alertVolume: Number(event.target.value) })} />
        <small>Message sounds and the ringtone. Separate from call volume.</small>
      </div>
      <label className="settings-checkbox">
        <input type="checkbox" checked={preferences.confirmCalls} onChange={(event) => preferences.setDevice({ confirmCalls: event.target.checked })} />
        Ask before starting a call
      </label>
      <label className="settings-checkbox">
        <input type="checkbox" checked={preferences.typingHidden} onChange={(event) => preferences.save({ typingHidden: event.target.checked })} />
        Turn off typing indicators (yours and everyone else&apos;s)
      </label>
      <div className="settings-retention">
        <h4>Keyword alerts</h4>
        <p>Alert me when these words appear, even in muted servers. Matched on this device, so the server never sees them.</p>
        <form className="friend-add" onSubmit={addWord}>
          <input aria-label="Keyword" maxLength={40} placeholder="Add a word" value={word} onChange={(event) => setWord(event.target.value)} />
          <button className="primary compact" disabled={!word.trim()}>Add</button>
        </form>
        <div className="keyword-list">
          {preferences.keywords.map((item) => <button key={item} className="secondary compact" title="Remove" onClick={() => preferences.setDevice({ keywords: preferences.keywords.filter((other) => other !== item) })}>{item} ×</button>)}
        </div>
      </div>
      {pushSupported() && (
        <div className="settings-retention">
          <h4>Alerts when Sonar is closed</h4>
          <p>Your browser says something arrived, without the sender or the text.</p>
          <button className="secondary" disabled={pushBusy} onClick={togglePush}>{push ? "Turn off closed-tab alerts" : "Turn on closed-tab alerts"}</button>
        </div>
      )}
      <div className="settings-retention">
        <h4>Blocked people</h4>
        <p>They cannot message, call or send you requests, and their server messages are hidden completely. They are not told.</p>
        {!preferences.blocked.length && <small>Nobody is blocked.</small>}
        {preferences.blocked.map((person) => (
          <div className="person friend-request" key={person._id}>
            <Avatar user={person} />
            <span><strong>{person.fullName}</strong>{person.username && <small>@{person.username}</small>}</span>
            <button className="secondary compact" onClick={() => preferences.unblock(person._id)}>Unblock</button>
          </div>
        ))}
      </div>
    </>
  );
}
