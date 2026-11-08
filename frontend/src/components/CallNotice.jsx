import { PhoneMissed } from "lucide-react";

// How a call nobody answered appears in the conversation.
export default function CallNotice({ mine, name }) {
  return (
    <div className={`call-notice ${mine ? "" : "missed"}`}>
      <PhoneMissed size={16} />
      <span>{mine ? "You called. No answer." : `Missed call from ${name.split(" ")[0]}`}</span>
    </div>
  );
}
