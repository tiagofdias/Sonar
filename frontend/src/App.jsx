import { useEffect } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { Toaster } from "react-hot-toast";
import { useAuthStore } from "./store/useAuthStore";
import HomePage from "./pages/HomePage";
import AuthPage from "./pages/AuthPage";
import AccountSettings from "./components/AccountSettings";
import PasskeyWindow from "./pages/PasskeyWindow";
import UnpairedDevicePage from "./pages/UnpairedDevicePage";
import RecoveryKeyPage from "./pages/RecoveryKeyPage";
export default function App() {
  const passkeyWindow = window.location.pathname === "/passkey";
  const { authUser, unpairedUser, unpairedReason, newRecoveryKey, checkAuth, isCheckingAuth, settingsOpen } = useAuthStore();
  useEffect(() => {
    if (passkeyWindow) return;
    const invite = new URLSearchParams(window.location.search).get("invite");
    if (invite && /^[a-f0-9]{48}$/i.test(invite))
      sessionStorage.setItem("sonar-invite", invite);
    checkAuth();
  }, [checkAuth, passkeyWindow]);
  if (passkeyWindow) return <PasskeyWindow />;
  if (isCheckingAuth)
    return (
      <div className="boot">
        <span className="sonar-mark" />
        Opening your private space...
      </div>
    );
  if (newRecoveryKey) return <><RecoveryKeyPage recoveryKey={newRecoveryKey} /><Toaster position="bottom-center" toastOptions={{ style: { background: "#24262b", color: "#eee" } }} /></>;
  if (unpairedUser) return <><UnpairedDevicePage user={unpairedUser} reason={unpairedReason} /><Toaster position="bottom-center" toastOptions={{ style: { background: "#24262b", color: "#eee" } }} /></>;
  return (
    <>
      <Routes>
        <Route
          path="/"
          element={authUser ? <HomePage /> : <Navigate to="/login" replace />}
        />
        <Route
          path="/login"
          element={authUser ? <Navigate to="/" replace /> : <AuthPage />}
        />
        <Route
          path="/recover"
          element={authUser ? <Navigate to="/" replace /> : <AuthPage recover />}
        />
        <Route
          path="/signup"
          element={authUser ? <Navigate to="/" replace /> : <AuthPage signup />}
        />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
      {authUser && settingsOpen && <AccountSettings />}
      <Toaster
        position="bottom-center"
        toastOptions={{ style: { background: "#24262b", color: "#eee" } }}
      />
    </>
  );
}
