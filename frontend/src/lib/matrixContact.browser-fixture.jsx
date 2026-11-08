import React from "react";
import { createRoot } from "react-dom/client";
import ContactVerification from "../components/ExperimentalContactVerification.jsx";

export function mountContactVerification(accountId, peerId, peerName) {
  const element = document.createElement("div");
  document.body.append(element);
  createRoot(element).render(React.createElement(ContactVerification, { accountId, peer: { _id: peerId, fullName: peerName } }));
}
