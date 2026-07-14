import activityTileArt from "../assets/activity-tile-art-v1.png";
import { ACTIVITY_COPY, ACTIVITY_ORDER } from "../lib/activities";
import type { Activity } from "../lib/types";

interface Props {
  disabled: boolean;
  onSelect: (activity: Activity) => Promise<void>;
}

export function ActivitySelector({ disabled, onSelect }: Props) {
  return (
    <section className="activity-tiles" aria-label="Choose a focus activity">
      {ACTIVITY_ORDER.map((activity) => {
        const copy = ACTIVITY_COPY[activity];
        return (
          <button
            key={activity}
            type="button"
            data-activity={activity}
            className="activity-tile"
            disabled={disabled}
            aria-label={`Start ${copy.label}`}
            onClick={() => void onSelect(activity)}
          >
            <span
              className="activity-tile-art"
              aria-hidden="true"
              style={{ backgroundImage: `url(${activityTileArt})` }}
            />
            <span className="activity-tile-label">{copy.label}</span>
          </button>
        );
      })}
    </section>
  );
}
