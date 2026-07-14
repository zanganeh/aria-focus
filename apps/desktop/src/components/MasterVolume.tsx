import { AppIcon } from "./AppIcon";

interface Props {
  value: number;
  pending: boolean;
  disabled: boolean;
  onChange: (volume: number) => void;
  variant?: "settings" | "compact";
}

const nextVolume = (input: HTMLInputElement) => Number(input.value);

/** Native range semantics keep keyboard volume changes accessible. */
export function MasterVolume({ value, pending, disabled, onChange, variant = "settings" }: Props) {
  if (variant === "compact") {
    return (
      <div className="player-volume" aria-busy={pending}>
        <AppIcon name={value === 0 ? "speaker-muted" : "speaker"} />
        <label className="visually-hidden" htmlFor="player-volume">
          Master volume
        </label>
        <input
          id="player-volume"
          type="range"
          min="0"
          max="100"
          step="1"
          value={value}
          disabled={disabled}
          aria-describedby="player-volume-status"
          onChange={(event) => onChange(nextVolume(event.currentTarget))}
        />
        <output htmlFor="player-volume" aria-hidden="true">
          {value}%
        </output>
        <span id="player-volume-status" className="visually-hidden" aria-live="polite">
          {pending ? "Saving volume…" : "Volume is saved on this device."}
        </span>
      </div>
    );
  }

  return (
    <section className="master-volume" aria-labelledby="master-volume-label">
      <label id="master-volume-label" htmlFor="master-volume">
        Master volume <output htmlFor="master-volume">{value}%</output>
      </label>
      <input
        id="master-volume"
        type="range"
        min="0"
        max="100"
        step="1"
        value={value}
        disabled={disabled}
        aria-describedby="master-volume-status"
        onChange={(event) => onChange(nextVolume(event.currentTarget))}
      />
      <p id="master-volume-status" aria-live="polite">
        {pending ? "Saving volume…" : "Volume is saved on this device."}
      </p>
    </section>
  );
}
