import { useEffect, useMemo, useState } from "react";
import { io } from "socket.io-client";
import { Toaster } from "react-hot-toast";
import { MonitorPlay } from "lucide-react";
import { axiosInstance as api } from "../lib/axios";
import { useWatch } from "../lib/useWatch";
import WatchStage from "../components/WatchStage";

// Opened by the desktop app with a one-time link. It never signs in to Sonar:
// the link becomes a cookie that only allows watching that one call or room.
export default function WatchPage() {
  const [phase, setPhase] = useState("opening");
  const [me, setMe] = useState(null);
  const [socket, setSocket] = useState(null);
  const watch = useWatch(socket, phase === "ready" || phase === "joined");

  useEffect(() => {
    let live = null, cancelled = false;
    (async () => {
      const token = new URLSearchParams(window.location.hash.slice(1)).get("t");
      // The token is single use: remove it from the address bar and history straight away.
      window.history.replaceState(null, "", "/watch");
      if (token) {
        try {
          const { data } = await api.post("/watch/redeem", { token });
          setMe(data.userId);
          sessionStorage.setItem("sonar-watch-user", data.userId);
        } catch {
          if (!cancelled) setPhase("expired");
          return;
        }
      } else setMe(sessionStorage.getItem("sonar-watch-user"));
      if (cancelled) return;
      live = io(window.location.origin, { withCredentials: true, auth: { watch: true }, transports: ["websocket"] });
      live.on("connect", () => { if (!cancelled) setPhase((value) => (value === "joined" ? value : "ready")); });
      live.on("connect_error", () => { if (!cancelled) setPhase("expired"); });
      setSocket(live);
    })();
    return () => { cancelled = true; live?.disconnect(); };
  }, []);
  useEffect(() => { if (phase === "joined" && !watch.joined && !watch.state) setPhase("ready"); }, [phase, watch.joined, watch.state]);

  const body = useMemo(() => {
    if (phase === "expired")
      return (
        <div className="watch-gate">
          <MonitorPlay size={34} />
          <strong>This watch link has expired</strong>
          <small>Open Watch together again from the Sonar app.</small>
        </div>
      );
    if (phase !== "joined")
      return (
        <div className="watch-gate">
          <MonitorPlay size={34} />
          <strong>Watch together</strong>
          <small>Your call stays in the Sonar app. The video plays here.</small>
          {/* A click lets the browser start the video with sound. */}
          <button className="primary" disabled={phase !== "ready"} onClick={async () => { await watch.join(); setPhase("joined"); }}>
            {phase === "ready" ? "Start watching" : "Connecting…"}
          </button>
        </div>
      );
    return <WatchStage watch={watch} me={me} standalone />;
  }, [phase, watch, me]);

  return (
    <main className="watch-page">
      <header><span className="sonar-mark" /> <strong>Sonar</strong> · Watch together</header>
      {body}
      <Toaster position="bottom-center" toastOptions={{ style: { background: "#24262b", color: "#eee" } }} />
    </main>
  );
}
