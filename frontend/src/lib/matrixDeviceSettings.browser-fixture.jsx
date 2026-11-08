import React from "react";
import { createRoot } from "react-dom/client";
import DeviceSettings from "../components/ExperimentalDeviceSettings.jsx";

export function mountDeviceSettings(accountId) {
  const container = document.createElement("div");
  document.body.append(container);
  createRoot(container).render(React.createElement(DeviceSettings, { accountId, onBusyChange() {} }));
}
