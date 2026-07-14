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
      <div className="genre-options">
        <label className={`genre-option${selected === null ? " selected" : ""}`}>
          <input
            type="radio"
            name="music-mood"
            checked={selected === null}
            onChange={() => onChange(null)}
          />
          Any compatible mood
        </label>
        {state?.available_moods.map((mood) => (
          <label key={mood.id} className={`genre-option${selected === mood.id ? " selected" : ""}`}>
            <input
              type="radio"
              name="music-mood"
              value={mood.id}
              checked={selected === mood.id}
              onChange={() => onChange(mood.id)}
            />
            {mood.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}
