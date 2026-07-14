import { useEffect, useRef, useState } from "react";
import { getItemFeedback, setItemFeedback } from "../lib/api";
import type {
  Activity,
  CurrentSource,
  ItemFeedbackState,
  TrackFeedback as Feedback,
  TrackEnjoyment,
} from "../lib/types";

const FOCUS_OPTIONS: ReadonlyArray<{ value: Feedback; label: string }> = [
  { value: "helps_focus", label: "Helps focus" },
  { value: "neutral", label: "Neutral" },
  { value: "distracting", label: "Distracting" },
];
const ENJOYMENT_OPTIONS: ReadonlyArray<{ value: TrackEnjoyment; label: string }> = [
  { value: "liked", label: "Liked" },
  { value: "not_for_me", label: "Not for me" },
];

export function TrackFeedback({
  source,
  activity,
  onError,
  onEnjoymentSaved,
}: {
  source: CurrentSource;
  activity: Activity;
  onError: (message: string) => void;
  onEnjoymentSaved?: () => void;
}) {
  const [state, setState] = useState<ItemFeedbackState | null>(null);
  const [saving, setSaving] = useState(false);
  const requestVersion = useRef(0);

  useEffect(() => {
    const version = ++requestVersion.current;
    setState(null);
    setSaving(false);
    void getItemFeedback(source.item_id)
      .then((next) => {
        if (
          version === requestVersion.current &&
          next.item_id === source.item_id &&
          next.activity === activity
        ) {
          setState(next);
          onEnjoymentSaved?.();
        }
      })
      .catch((error: unknown) => {
        if (version === requestVersion.current) {
          onError(
            `Unable to load track feedback: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
      });
  }, [activity, onError, onEnjoymentSaved, source.item_id]);

  const save = (focusFeedback: Feedback | null, enjoyment: TrackEnjoyment | null) => {
    if (saving) return;
    const version = requestVersion.current;
    setSaving(true);
    void setItemFeedback(source.item_id, focusFeedback, enjoyment)
      .then((next) => {
        if (
          version === requestVersion.current &&
          next.item_id === source.item_id &&
          next.activity === activity
        ) {
          setState(next);
        }
      })
      .catch((error: unknown) => {
        if (version === requestVersion.current) {
          onError(
            `Unable to save track feedback: ${error instanceof Error ? error.message : String(error)}. Your choice was not saved; try again.`,
          );
        }
      })
      .finally(() => {
        if (version === requestVersion.current) setSaving(false);
      });
  };

  return (
    <fieldset className="track-feedback" disabled={saving} aria-busy={saving}>
      <legend>Track feedback for {activity.replace("_", " ")}</legend>
      <p className="track-feedback-note">Focus and sound preference are separate.</p>
      <fieldset className="track-feedback-question">
        <legend>Did this help you focus?</legend>
        <div className="track-feedback-options">
          {FOCUS_OPTIONS.map((option) => (
            <label key={option.value}>
              <input
                type="radio"
                name={`track-focus-feedback-${source.item_id}`}
                checked={state?.focus_feedback === option.value}
                onChange={() => save(option.value, state?.enjoyment ?? null)}
              />
              {option.label}
            </label>
          ))}
          <button type="button" onClick={() => save(null, state?.enjoyment ?? null)}>
            Clear
          </button>
        </div>
      </fieldset>
      <fieldset className="track-feedback-question">
        <legend>Did you like the sound?</legend>
        <div className="track-feedback-options">
          {ENJOYMENT_OPTIONS.map((option) => (
            <label key={option.value}>
              <input
                type="radio"
                name={`track-enjoyment-${source.item_id}`}
                checked={state?.enjoyment === option.value}
                onChange={() => save(state?.focus_feedback ?? null, option.value)}
              />
              {option.label}
            </label>
          ))}
          <button type="button" onClick={() => save(state?.focus_feedback ?? null, null)}>
            Clear
          </button>
        </div>
      </fieldset>
    </fieldset>
  );
}
