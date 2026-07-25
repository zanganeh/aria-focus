import type { ActivityGenreState } from "../lib/types";

interface Props {
  state: ActivityGenreState | null;
  disabled: boolean;
  onChange: (genreId: string | null) => void;
}

export function GenreSelector({ state, disabled, onChange }: Props) {
  const selected = state?.selected_genre_id ?? null;
  const unavailable = Boolean(selected && state && !state.selected_genre_available);
  return (
    <fieldset className="genre-selector" disabled={disabled} aria-describedby="genre-help">
      <legend>Music genre</legend>
      <p id="genre-help" className="genre-help">
        Choose music you enjoy for this activity. It filters playback, not just the display.
      </p>
      {unavailable && (
        <p className="genre-unavailable" role="status">
          Saved genre “{selected}” is unavailable. Choose Any compatible genre or an available
          option.
        </p>
      )}
      <label className="genre-select-label">
        <span className="visually-hidden">Choose a music genre</span>
        <select
          aria-label="Choose a music genre"
          value={selected ?? ""}
          onChange={(event) => onChange(event.target.value || null)}
        >
          <option value="">Any compatible genre</option>
          {state?.available_genres.map((genre) => (
            <option key={genre.id} value={genre.id}>
              {genre.label}
            </option>
          ))}
        </select>
      </label>
    </fieldset>
  );
}
