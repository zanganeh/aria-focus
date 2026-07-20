import type { Intensity } from "../lib/types";

interface Props {
  value: Intensity;
  disabled: boolean;
  onChange: (value: Intensity) => void;
}

/** A one-tap shortcut for the existing High / ADHD stimulation level. */
export function AdhdModeToggle({ value, disabled, onChange }: Props) {
  const enabled = value === "high";

  return (
    <button
      type="button"
      className={`adhd-mode-toggle${enabled ? " enabled" : ""}`}
      disabled={disabled}
      aria-pressed={enabled}
      onClick={() => onChange(enabled ? "medium" : "high")}
    >
      <span className="adhd-mode-copy">
        <strong>ADHD mode</strong>
        <small>{enabled ? "High stimulation" : "Off"}</small>
      </span>
      <span className="adhd-mode-switch" aria-hidden="true">
        <span />
      </span>
    </button>
  );
}
