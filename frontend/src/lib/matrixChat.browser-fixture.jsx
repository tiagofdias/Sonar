import React from "react";
import { createRoot } from "react-dom/client";
import MatrixChat from "../components/ExperimentalMatrixChat.jsx";
export function mountMatrixChat(accountId, peerId, peerName) {
  const element = document.createElement("div"); document.body.append(element);
  const listeners = new Map();
  const socket = { on(name, handler) { listeners.set(name, handler); }, off(name, handler) { if (listeners.get(name) === handler) listeners.delete(name); } };
  window.testMatrixSocketEvent = name => listeners.get(name)?.();
  createRoot(element).render(React.createElement(MatrixChat, { accountId, peer: { _id: peerId, fullName: peerName }, socket }));
}
