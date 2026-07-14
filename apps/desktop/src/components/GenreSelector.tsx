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
      <div className="genre-options">
        <label className={`genre-option${selected === null ? " selected" : ""}`}>
          <input
            type="radio"
            name="music-genre"
            checked={selected === null}
            onChange={() => onChange(null)}
          />
          Any compatible genre
        </label>
        {state?.available_genres.map((genre) => (
          <label
            key={genre.id}
            className={`genre-option${selected === genre.id ? " selected" : ""}`}
          >
            <input
              type="radio"
              name="music-genre"
              value={genre.id}
              checked={selected === genre.id}
              onChange={() => onChange(genre.id)}
            />
            {genre.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}
