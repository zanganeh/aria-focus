import type { ActivityMoodState } from "../lib/types";

interface Props {
  state: ActivityMoodState | null;
  disabled: boolean;
  onChange: (moodId: string | null) => void;
}

export function MoodSelector({ state, disabled, onChange }: Props) {
  const selected = state?.selected_mood_id ?? null;
  const unavailable = Boolean(selected && state && !state.selected_mood_available);
  return (
    <fieldset className="genre-selector" disabled={disabled} aria-describedby="mood-help">
      <legend>Mood</legend>
      <p id="mood-help" className="genre-help">
        Choose a mood for this activity. It filters playback, not just the display.
      </p>
      {unavailable && (
        <p className="genre-unavailable" role="status">
          Saved mood “{selected}” is unavailable for this genre. Choose Any compatible mood or an
          available option.
        </p>
      )}
      <label className="genre-select-label">
        <span className="visually-hidden">Choose a mood</span>
        <select
          aria-label="Choose a mood"
          value={selected ?? ""}
          onChange={(event) => onChange(event.target.value || null)}
        >
          <option value="">Any compatible mood</option>
          {state?.available_moods.map((mood) => (
            <option key={mood.id} value={mood.id}>
              {mood.label}
            </option>
          ))}
        </select>
      </label>
    </fieldset>
  );
}
