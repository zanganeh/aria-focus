import { useEffect, useState } from "react";
import type { SessionType } from "../lib/types";
import { AppIcon } from "./AppIcon";

const PRESETS = [15, 25, 30, 45, 60, 90] as const;

interface Props {
  value: SessionType;
  disabled: boolean;
  onChange: (value: SessionType) => void;
}

export function SessionTypeSelector({ value, disabled, onChange }: Props) {
  const [customMinutes, setCustomMinutes] = useState(25);
  const [customSelected, setCustomSelected] = useState(false);
  const [workMinutes, setWorkMinutes] = useState(25);
  const [breakMinutes, setBreakMinutes] = useState(5);
  const [repeats, setRepeats] = useState(4);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (value.kind === "countdown") {
      const minutes = value.seconds / 60;
      setCustomMinutes(minutes);
      setCustomSelected(!PRESETS.includes(minutes as (typeof PRESETS)[number]));
    }
    if (value.kind === "interval") {
      setWorkMinutes(value.work_seconds / 60);
      setBreakMinutes(value.break_seconds / 60);
      setRepeats(value.repeats);
    }
    setError(null);
  }, [value]);

  const countdownMinutes = value.kind === "countdown" ? value.seconds / 60 : 25;
  const countdownChoice =
    !customSelected && PRESETS.includes(countdownMinutes as (typeof PRESETS)[number])
      ? String(countdownMinutes)
      : "custom";

  function selectKind(kind: SessionType["kind"]) {
    setError(null);
    if (kind === "infinite") onChange({ kind: "infinite" });
    if (kind === "countdown") onChange({ kind: "countdown", seconds: 25 * 60 });
    if (kind === "interval") {
      onChange({ kind: "interval", work_seconds: 25 * 60, break_seconds: 5 * 60, repeats: 4 });
    }
  }

  function applyCountdown() {
    if (!Number.isInteger(customMinutes) || customMinutes < 1 || customMinutes > 480) {
      setError("Countdown must be a whole number from 1 to 480 minutes.");
      return;
    }
    setError(null);
    onChange({ kind: "countdown", seconds: customMinutes * 60 });
  }

  function applyInterval() {
    const valid =
      Number.isInteger(workMinutes) &&
      workMinutes >= 1 &&
      workMinutes <= 240 &&
      Number.isInteger(breakMinutes) &&
      breakMinutes >= 1 &&
      breakMinutes <= 60 &&
      Number.isInteger(repeats) &&
      repeats >= 1 &&
      repeats <= 12 &&
      workMinutes * repeats + breakMinutes * (repeats - 1) <= 720;
    if (!valid) {
      setError(
        "Use whole minutes: work 1–240, break 1–60, rounds 1–12, no more than 12 hours total.",
      );
      return;
    }
    setError(null);
    onChange({
      kind: "interval",
      work_seconds: workMinutes * 60,
      break_seconds: breakMinutes * 60,
      repeats,
    });
  }

  const options = [
    {
      kind: "infinite" as const,
      label: "Infinite",
      description: "Play until you stop",
      icon: "infinity" as const,
    },
    {
      kind: "countdown" as const,
      label: "Countdown",
      description: "Choose a duration",
      icon: "clock" as const,
    },
    {
      kind: "interval" as const,
      label: "Interval",
      description: "Work and quiet breaks",
      icon: "repeat" as const,
    },
  ];

  return (
    <fieldset className="session-type" disabled={disabled}>
      <legend>Session timer</legend>
      <div className="session-type-options">
        {options.map(({ kind, label, description, icon }) => (
          <label key={kind} className={`session-type-card${value.kind === kind ? " selected" : ""}`}>
            <input
              className="session-type-input"
              type="radio"
              name="session-type"
              value={kind}
              checked={value.kind === kind}
              aria-label={label}
              onChange={() => selectKind(kind)}
            />
            <span className="session-type-icon" aria-hidden="true">
              <AppIcon name={icon} />
            </span>
            <span className="session-type-copy">
              <strong>{label}</strong>
              <small>{description}</small>
            </span>
          </label>
        ))}
      </div>

      {value.kind === "countdown" && (
        <div className="timer-fields">
          <label>
            Duration
            <select
              aria-label="Countdown duration"
              value={countdownChoice}
              onChange={(event) => {
                if (event.target.value === "custom") {
                  setCustomSelected(true);
                  return;
                }
                const minutes = Number(event.target.value);
                setCustomSelected(false);
                setCustomMinutes(minutes);
                onChange({ kind: "countdown", seconds: minutes * 60 });
              }}
            >
              {PRESETS.map((minutes) => (
                <option key={minutes} value={minutes}>
                  {minutes} minutes
                </option>
              ))}
              <option value="custom">Custom</option>
            </select>
          </label>
          {countdownChoice === "custom" && (
            <>
              <label>
                Custom minutes
                <input
                  aria-label="Custom countdown minutes"
                  type="number"
                  min="1"
                  max="480"
                  step="1"
                  value={customMinutes}
                  onChange={(event) => setCustomMinutes(Number(event.target.value))}
                />
              </label>
              <button type="button" onClick={applyCountdown}>
                Apply countdown
              </button>
            </>
          )}
        </div>
      )}

      {value.kind === "interval" && (
        <div className="timer-fields interval-fields">
          <label>
            Work minutes
            <input
              aria-label="Interval work minutes"
              type="number"
              min="1"
              max="240"
              step="1"
              value={workMinutes}
              onChange={(event) => setWorkMinutes(Number(event.target.value))}
            />
          </label>
          <label>
            Break minutes
            <input
              aria-label="Interval break minutes"
              type="number"
              min="1"
              max="60"
              step="1"
              value={breakMinutes}
              onChange={(event) => setBreakMinutes(Number(event.target.value))}
            />
          </label>
          <label>
            Rounds
            <input
              aria-label="Interval rounds"
              type="number"
              min="1"
              max="12"
              step="1"
              value={repeats}
              onChange={(event) => setRepeats(Number(event.target.value))}
            />
          </label>
          <button type="button" onClick={applyInterval}>
            Apply interval
          </button>
          <p>Breaks are silent in this version.</p>
        </div>
      )}
      {error && <p role="alert">{error}</p>}
      {disabled && <p className="timer-note">Stop the session to change its timer.</p>}
    </fieldset>
  );
}
