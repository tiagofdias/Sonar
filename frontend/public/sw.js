// Shows alerts while Sonar is closed. The server only ever says that something arrived.
self.addEventListener("push", (event) => {
  let data = {};
  try { data = event.data.json(); } catch { /* Fall back to the plain text below. */ }
  const body = data.body === "Incoming call" ? "Incoming call" : "New message";
  event.waitUntil(self.registration.showNotification("Sonar", { body, tag: "sonar-alert", icon: "/sonar-mark.svg" }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((list) => {
    const open = list.find((client) => "focus" in client);
    return open ? open.focus() : self.clients.openWindow("/");
  }));
});
