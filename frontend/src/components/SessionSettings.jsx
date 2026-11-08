import { useEffect, useState } from "react";
import { Monitor, LogOut } from "lucide-react";
import toast from "react-hot-toast";
import { axiosInstance as api } from "../lib/axios";
import { useAuthStore } from "../store/useAuthStore";

export default function SessionSettings() {
  const [data, setData] = useState(null), [loading, setLoading] = useState(true), [busy, setBusy] = useState(null), [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    api.get("/auth/sessions").then(result => { if (active) setData(result.data); })
      .catch(() => { if (active) setError("Could not load sessions. Close settings and try again."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  const revoke = async (session) => {
    if (busy || !window.confirm(session.current ? "Sign out this session?" : "Sign out this session on its browser?")) return;
    setBusy(session.id);
    try {
      const result = await api.delete(`/auth/sessions/${session.id}`);
      if (result.data.current) useAuthStore.getState().clearSession();
      else { setData(current => ({ ...current, sessions: current.sessions.filter(item => item.id !== session.id) })); toast.success("Session signed out"); }
    } catch (err) { toast.error(err.response?.data?.message || "Could not sign out that session"); }
    finally { setBusy(null); }
  };
  return <section className="security-card">
    <div className="security-card-heading"><span className="security-icon"><Monitor size={20} /></span><div><h4>Signed-in sessions</h4><p>Review sign-ins and sign out individual sessions.</p></div></div>
    {loading ? <p className="settings-caption">Loading sessions...</p> : error ? <p role="status">{error}</p> : <>
      {data.legacyCurrent && <p className="settings-caption">This older session is not listed. Use Sign out everywhere to revoke older sessions.</p>}
      {!data.sessions.length && <p className="settings-caption">No tracked sessions found.</p>}
      <ul className="session-list">{data.sessions.map(item => <li key={item.id}>
        <div><strong>{item.method === "passkey" ? "Passkey sign-in" : "Password sign-in"}{item.current ? " · This session" : ""}</strong>
          <small>Signed in {new Date(item.createdAt).toLocaleString()} · Expires {new Date(item.expiresAt).toLocaleDateString()}</small></div>
        <button className="secondary" disabled={Boolean(busy)} onClick={() => revoke(item)}><LogOut size={14} /> {busy === item.id ? "Signing out..." : "Sign out"}</button>
      </li>)}</ul>
      {data.hasMore && <p className="settings-caption">Showing the 100 newest sessions. Use Sign out everywhere to revoke all sessions.</p>}
      <p className="settings-caption">Session entries don’t include device names or IP addresses. Signing out does not erase encryption keys from a browser.</p>
    </>}
  </section>;
}
