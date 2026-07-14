import { formatDuration } from "../lib/format";
import type { SessionSnapshot } from "../lib/types";

interface Props {
  snapshot: SessionSnapshot | null;
}

export function SessionTimer({ snapshot }: Props) {
  const focusElapsed = snapshot?.focus_elapsed_seconds ?? 0;
  const phaseRemaining = snapshot?.current_phase_remaining_seconds ?? null;
  const totalRemaining = snapshot?.total_remaining_seconds ?? null;
  const status = snapshot?.status ?? "idle";
  const phase = snapshot?.phase ?? null;
  const isInterval = snapshot?.kind.kind === "interval";
  const mainSeconds = phaseRemaining ?? focusElapsed;
  const mainLabel =
    phaseRemaining === null
      ? "Focus time"
      : phase === "break"
        ? "Break remaining"
        : "Work remaining";

  return (
    <section className={`timer timer-${status}`} aria-live="polite">
      <div className="elapsed" aria-label={mainLabel}>
        {formatDuration(mainSeconds)}
      </div>
      {phase && (
        <div className="phase">
          {phase === "break" ? "Silent break" : "Work"}
          {isInterval && snapshot?.current_round !== null && (
            <>
              {" "}
              · Round {snapshot?.current_round} of {snapshot?.total_rounds}
            </>
          )}
        </div>
      )}
      <div className="focus-elapsed" aria-label="Elapsed focus work">
        Focus {formatDuration(focusElapsed)}
      </div>
      {totalRemaining !== null && (
        <div className="remaining" aria-label="Total session remaining">
          Total remaining {formatDuration(totalRemaining)}
        </div>
      )}
      {status === "expired" && <div className="completion">Session complete.</div>}
      <div className="status" data-status={status}>
        {status}
      </div>
    </section>
  );
}
