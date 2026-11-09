export const PRESENCE_OPTIONS = [
  ["online", "Online"],
  ["away", "Away"],
  ["busy", "Busy"],
  ["offline", "Appear offline"],
];

export const presenceLabel = (value) => value === "room" ? "In a voice room" : PRESENCE_OPTIONS.find(([key]) => key === value)?.[1] || "Online";
