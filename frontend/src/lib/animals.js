// Keep this list and its order the same in backend/src/lib/animals.js and desktop/src/theme.rs.
export const ANIMALS = ["whale", "shark", "turtle", "dolphin", "octopus", "jellyfish", "seahorse", "crab", "pufferfish", "manta", "orca", "seal", "squid", "starfish", "clownfish", "lobster"];
export const animalName = (animal) => (animal === "manta" ? "Manta ray" : animal[0].toUpperCase() + animal.slice(1));
// Someone who has not chosen always gets the same animal, on every device.
export function animalOf(user) {
  if (ANIMALS.includes(user?.animal)) return user.animal;
  const number = parseInt(String(user?._id || "").slice(-8), 16);
  return ANIMALS[(Number.isNaN(number) ? 0 : number) % ANIMALS.length];
}
