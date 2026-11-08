// Revalidate idle sockets as well as incoming events. The database is the
// authority across processes; polling does not provide instant revocation.
export function watchSocketSession(socket, verify, intervalMs = 15000, timeoutMs = 5000) {
  let checking = false;
  let stopped = false;
  const revoke = () => {
    if (stopped) return;
    stopped = true;
    clearInterval(timer);
    socket.emit("sessionRevoked");
    socket.disconnect(true);
  };
  const check = async () => {
    if (stopped || checking) return;
    checking = true;
    let timeout;
    try {
      const deadline = new Promise((_, reject) => {
        timeout = setTimeout(() => reject(new Error("Session verification timed out")), timeoutMs);
        timeout.unref?.();
      });
      if (!(await Promise.race([verify(), deadline]))) revoke();
    } catch {
      // If the authority is unavailable, do not leave the socket receiving data.
      revoke();
    } finally { clearTimeout(timeout); checking = false; }
  };
  const timer = setInterval(check, intervalMs);
  timer.unref?.();
  socket.once("disconnect", () => { stopped = true; clearInterval(timer); });
  return { check, stop: () => { stopped = true; clearInterval(timer); } };
}
