import { useAuthStore } from "../store/useAuthStore";
import { animalOf } from "../lib/animals";

export function AnimalFace({ animal }) {
  const image = `url(/animals/${animal}.svg)`;
  return <span className="animal-face" style={{ WebkitMaskImage: image, maskImage: image }} />;
}

// A person's sea animal. `dot` shows a status circle: true uses their live status.
export default function Avatar({ user, dot, large }) {
  const status = useAuthStore((state) => state.presence[user?._id]);
  const shown = typeof dot === "string" ? dot : status;
  return (
    <span className={`avatar-tile ${large ? "large" : ""}`}>
      <AnimalFace animal={animalOf(user)} />
      {dot && <i className={shown ? `status-${shown}` : undefined} />}
    </span>
  );
}
