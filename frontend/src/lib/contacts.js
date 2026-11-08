// Resolve a selected contact from the most recent directory response. Never
// retain stale key material after a contact has disappeared from the list.
export function refreshedContact(selected, users) {
  if (!selected) return null;
  return users.find(user => user._id === selected._id) ?? null;
}
