import { useState } from "react";
import type {
  SessionFocusOutcome,
  SessionHistoryRecord,
  SessionSoundEnjoyment,
} from "../lib/types";

export function SessionEndCard({
  session,
  onSave,
  onSkip,
}: {
  session: SessionHistoryRecord;
  onSave: (
    focus: SessionFocusOutcome | null,
    enjoyment: SessionSoundEnjoyment | null,
  ) => Promise<void>;
  onSkip: () => void;
}) {
  const [focus, setFocus] = useState<SessionFocusOutcome | null>(session.focus_outcome);
  const [enjoyment, setEnjoyment] = useState<SessionSoundEnjoyment | null>(session.sound_enjoyment);
  const [saving, setSaving] = useState(false);
  return (
    <section className="session-end-card" aria-labelledby="session-end-title">
      <h2 id="session-end-title">How was that session?</h2>
      <fieldset>
        <legend>Focus outcome (optional)</legend>
        {(["helped_focus", "neutral", "distracting"] as const).map((value) => (
          <label key={value}>
            <input
              type="radio"
              name="session-focus"
              checked={focus === value}
              onChange={() => setFocus(value)}
            />
            {value.replaceAll("_", " ")}
          </label>
        ))}
        <button type="button" onClick={() => setFocus(null)}>
          Clear focus outcome
        </button>
      </fieldset>
      <fieldset>
        <legend>Sound enjoyment (optional)</legend>
        {(["liked", "not_for_me"] as const).map((value) => (
          <label key={value}>
            <input
              type="radio"
              name="session-enjoyment"
              checked={enjoyment === value}
              onChange={() => setEnjoyment(value)}
            />
            {value.replaceAll("_", " ")}
          </label>
        ))}
        <button type="button" onClick={() => setEnjoyment(null)}>
          Clear sound enjoyment
        </button>
      </fieldset>
      <button
        type="button"
        disabled={saving}
        onClick={() => {
          setSaving(true);
          void onSave(focus, enjoyment).finally(() => setSaving(false));
        }}
      >
        Save
      </button>
      <button type="button" disabled={saving} onClick={onSkip}>
        Skip
      </button>
    </section>
  );
}
