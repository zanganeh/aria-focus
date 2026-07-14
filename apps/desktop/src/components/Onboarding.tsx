import { useState } from "react";
import type { Intensity } from "../lib/types";

const GENRES = [
  "Atmospheric",
  "Lo-Fi",
  "Electronic",
  "Piano",
  "Classical",
  "Acoustic",
  "Cinematic",
  "Drone",
  "Grooves",
  "Post-Rock",
  "Nature",
] as const;
const genreId = (genre: string) => genre.toLowerCase().replace("-", "_").replace(" ", "_");

export function Onboarding({
  onComplete,
}: {
  onComplete: (intensity: Exclude<Intensity, "off">, genres: string[]) => Promise<void>;
}) {
  const [intensity, setIntensity] = useState<Exclude<Intensity, "off">>("medium");
  const [genres, setGenres] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toggle = (genre: string) =>
    setGenres((current) =>
      current.includes(genre)
        ? current.filter((value) => value !== genre)
        : current.length < 3
          ? [...current, genre].sort()
          : current,
    );
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onComplete(intensity, genres.map(genreId));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <main className="app onboarding" aria-labelledby="onboarding-title">
      <h1 id="onboarding-title">Set a starting sound preference</h1>
      <p>
        Functional audio is flexible background sound for focus. This is a personal preference, not
        medical advice.
      </p>
      <fieldset>
        <legend>Starting stimulation</legend>
        <label>
          <input
            type="radio"
            name="stimulation"
            checked={intensity === "low"}
            onChange={() => setIntensity("low")}
          />{" "}
          Sound-sensitive — Low
        </label>
        <label>
          <input
            type="radio"
            name="stimulation"
            checked={intensity === "medium"}
            onChange={() => setIntensity("medium")}
          />{" "}
          Ordinary stimulation — Medium
        </label>
        <label>
          <input
            type="radio"
            name="stimulation"
            checked={intensity === "high"}
            onChange={() => setIntensity("high")}
          />{" "}
          Strong stimulation — High
        </label>
      </fieldset>
      <fieldset>
        <legend>Preferred genres (optional, up to 3)</legend>
        {GENRES.map((genre) => (
          <label key={genre}>
            <input
              type="checkbox"
              checked={genres.includes(genre)}
              disabled={!genres.includes(genre) && genres.length === 3}
              onChange={() => toggle(genre)}
            />{" "}
            {genre}
          </label>
        ))}
      </fieldset>
      {error && <p role="alert">Couldn’t start your session: {error}</p>}
      <button type="button" disabled={busy} onClick={() => void submit()}>
        {busy ? "Starting…" : "Start 30-minute Deep Work"}
      </button>
    </main>
  );
}
