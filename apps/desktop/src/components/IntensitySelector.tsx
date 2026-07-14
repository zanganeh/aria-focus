import { INTENSITY_ARIA, INTENSITY_LABELS, INTENSITY_ORDER } from "../lib/format";
import { PRODUCT_COPY } from "../lib/copy";
import type { Intensity } from "../lib/types";

interface Props {
  value: Intensity;
  disabled: boolean;
  onChange: (i: Intensity) => void;
}

/** Radio-group intensity selector with non-colour level indicators. */
export function IntensitySelector({ value, disabled, onChange }: Props) {
  return (
    <fieldset className="intensity" disabled={disabled}>
      <legend>Stimulation intensity</legend>
      <div className="intensity-options">
        {INTENSITY_ORDER.map((i) => {
          const checked = i === value;
          return (
            <label key={i} className={`intensity-btn${checked ? " selected" : ""}`}>
              <input
                className="intensity-input"
                type="radio"
                name="stimulation-intensity"
                value={i}
                checked={checked}
                aria-label={`${INTENSITY_LABELS[i]}. ${INTENSITY_ARIA[i]}`}
                onChange={() => onChange(i)}
              />
              <span className="level" aria-hidden="true">
                L{INTENSITY_ORDER.indexOf(i)}
              </span>
              <span className="label">{INTENSITY_LABELS[i]}</span>
            </label>
          );
        })}
      </div>
      <p className="intensity-note">{PRODUCT_COPY.intensityNote}</p>
    </fieldset>
  );
}
