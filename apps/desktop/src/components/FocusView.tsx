import { useEffect, useRef } from "react";
import { formatDuration } from "../lib/format";
import type { SessionSnapshot } from "../lib/types";

interface Props {
  snapshot: SessionSnapshot;
  activityLabel: string;
  coverArt?: string | null;
  onPause: () => void;
  onResume: () => void;
  onExit: () => void;
}

/** A deliberately small session-only surface for sustained focus. */
export function FocusView({ snapshot, activityLabel, coverArt, onPause, onResume, onExit }: Props) {
  const primaryControl = useRef<HTMLButtonElement>(null);
  const isInfinite = snapshot.kind.kind === "infinite";
  const isInterval = snapshot.kind.kind === "interval";
  const remaining = snapshot.current_phase_remaining_seconds;
  const timeLabel = isInfinite
    ? "Focused"
    : snapshot.phase === "break"
      ? "Break remaining"
      : "Work remaining";
  const seconds = isInfinite ? snapshot.focus_elapsed_seconds : (remaining ?? 0);

  useEffect(() => {
    primaryControl.current?.focus();
  }, []);

  return (
    <main className="focus-view" aria-label="Focus view">
      {coverArt && (
        <img className="focus-view-background" src={coverArt} alt="" aria-hidden="true" />
      )}
      <div className="focus-view-overlay" aria-hidden="true" />
      <section
        className="focus-view-content"
        aria-labelledby="focus-view-activity"
        onKeyDown={(event) => {
          if (event.key === "Escape") onExit();
        }}
      >
        <h1 id="focus-view-activity">{activityLabel}</h1>
        {isInterval && snapshot.phase && (
          <p className="focus-view-phase">{snapshot.phase === "break" ? "Break" : "Work"}</p>
        )}
        <div className="focus-view-time" aria-label={`${timeLabel}: ${formatDuration(seconds)}`}>
          <span>{formatDuration(seconds)}</span>
          <small>{timeLabel}</small>
        </div>
        <div className="focus-view-actions">
          <button
            ref={primaryControl}
            type="button"
            className="primary"
            onClick={snapshot.status === "playing" ? onPause : onResume}
          >
            {snapshot.status === "playing" ? "Pause" : "Resume"}
          </button>
          <button type="button" onClick={onExit}>
            Exit focus view
          </button>
        </div>
      </section>
    </main>
  );
}
