import activityTileArt from "../assets/activity-tile-art-v1.png";
import type { Activity } from "../lib/types";

interface Props {
  activity: Activity;
  className?: string;
  label?: string;
  decorative?: boolean;
}

/** Reuses the approved activity artwork wherever a track has no cover art. */
export function ActivityArtwork({
  activity,
  className = "",
  label,
  decorative = true,
}: Props) {
  return (
    <span
      className={`activity-artwork${className ? ` ${className}` : ""}`}
      data-activity={activity}
      aria-hidden={decorative ? "true" : undefined}
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : label}
      style={{ backgroundImage: `url(${activityTileArt})` }}
    />
  );
}
