import { AppIcon } from "./AppIcon";

interface Props {
  status: string;
  starting: boolean;
  activityLabel: string;
  startDisabled?: boolean;
  actionsDisabled?: boolean;
  onStart: () => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
  navigationAvailable?: boolean;
  navigationPending?: boolean;
  onNext?: () => void;
  onPrevious?: () => void;
}

/** Transport controls. The default path is one click: Start. */
export function TransportControls({
  status,
  starting,
  activityLabel,
  startDisabled = false,
  actionsDisabled = false,
  onStart,
  onPause,
  onResume,
  onStop,
  navigationAvailable = false,
  navigationPending = false,
  onNext,
  onPrevious,
}: Props) {
  const active = status === "playing" || status === "paused";
  const navigationDisabled = !navigationAvailable || navigationPending || actionsDisabled;

  return (
    <section className={`transport${active ? " transport-active" : ""}`}>
      {!active && (
        <button
          type="button"
          className="primary"
          onClick={onStart}
          disabled={starting || startDisabled}
          aria-label={`Start ${activityLabel} focus session`}
        >
          {starting ? "Starting…" : `Start ${activityLabel}`}
        </button>
      )}
      {active && (
        <div className="transport-media" role="group" aria-label="Playback controls">
          <button
            type="button"
            className="transport-icon transport-prev"
            onClick={() => onPrevious?.()}
            disabled={navigationDisabled}
            aria-label="Previous installed track"
          >
            <AppIcon name="previous" />
          </button>
          <button
            type="button"
            className="transport-playpause"
            onClick={status === "playing" ? onPause : onResume}
            disabled={actionsDisabled}
            aria-label={status === "playing" ? "Pause session" : "Resume session"}
          >
            <AppIcon name={status === "playing" ? "pause" : "play"} />
          </button>
          <button
            type="button"
            className="transport-icon transport-next"
            onClick={() => onNext?.()}
            disabled={navigationDisabled}
            aria-label="Next installed track"
          >
            <AppIcon name="next" />
          </button>
          <button
            type="button"
            className="transport-icon transport-stop"
            onClick={onStop}
            disabled={actionsDisabled}
            aria-label="Stop session"
          >
            <AppIcon name="stop" />
          </button>
        </div>
      )}
    </section>
  );
}
