import { useEffect, useMemo, useState } from "react";
import {
  buildReviewExport,
  loadReviewRatings,
  REVIEW_NOTE_MAX_LENGTH,
  reviewNamespaceForAliases,
  saveReviewRatings,
  type ReviewRatingChoice,
  type ReviewRatings,
} from "../lib/reviewRatings";
import type { ReviewCandidate } from "../lib/types";

interface Props {
  candidates: ReviewCandidate[];
  active: boolean;
  disabled: boolean;
  onStart: (id: string) => void;
}

const ROUND_ONE_ALIASES = new Set(["I", "J", "N", "O", "Q", "R", "U", "X"]);

export function QuarantinedReview({ candidates, active, disabled, onStart }: Props) {
  if (candidates.length === 0) return null;

  return (
    <QuarantinedReviewContent
      candidates={candidates}
      active={active}
      disabled={disabled}
      onStart={onStart}
    />
  );
}

function QuarantinedReviewContent({ candidates, active, disabled, onStart }: Props) {
  const hasRoundOneShortlist = useMemo(
    () =>
      [...ROUND_ONE_ALIASES].every((alias) =>
        candidates.some((candidate) => candidate.alias === alias),
      ),
    [candidates],
  );
  const [showAllCandidates, setShowAllCandidates] = useState(false);
  const visibleCandidates = useMemo(
    () =>
      hasRoundOneShortlist && !showAllCandidates
        ? candidates.filter((candidate) => ROUND_ONE_ALIASES.has(candidate.alias))
        : candidates,
    [candidates, hasRoundOneShortlist, showAllCandidates],
  );
  const aliases = useMemo(
    () => visibleCandidates.map((candidate) => candidate.alias),
    [visibleCandidates],
  );
  const namespace = useMemo(() => reviewNamespaceForAliases(aliases), [aliases]);
  const [ratings, setRatings] = useState<ReviewRatings>(() =>
    loadReviewRatings(namespace, aliases),
  );
  const [exportText, setExportText] = useState("");
  const [copyMessage, setCopyMessage] = useState("");

  useEffect(() => {
    setRatings(loadReviewRatings(namespace, aliases));
    setExportText("");
    setCopyMessage("");
  }, [aliases, namespace]);

  const updateRating = (alias: string, choice: ReviewRatingChoice, note: string) => {
    const next = { ...ratings, [alias]: { choice, note: note.slice(0, REVIEW_NOTE_MAX_LENGTH) } };
    setRatings(next);
    saveReviewRatings(namespace, aliases, next);
  };

  const exportRatings = async () => {
    const nextExport = buildReviewExport(namespace, aliases, ratings);
    setExportText(nextExport);
    try {
      await navigator.clipboard.writeText(nextExport);
      setCopyMessage("Copied review JSON to the clipboard.");
    } catch {
      setCopyMessage("Could not copy automatically. The JSON below remains selectable to copy.");
    }
  };

  return (
    <section aria-labelledby="review-heading" className="quarantined-review">
      <h2 id="review-heading">Quarantined candidate review</h2>
      <p>
        Blind-triage evidence only. This is not representative-work-session QA, not a second
        reviewer, and not publication or private-beta approval.
      </p>
      <p>
        Each 90-second source repeats with a provisional boundary crossfade, not an authored safe
        loop. During the 45–90 minute session, notice and report every repeated transition.
      </p>
      <p>
        {active
          ? "Stop the current session before switching candidates."
          : "Select a visible blind track, then start a review session."}
      </p>
      {hasRoundOneShortlist && (
        <div className="review-scope" aria-label="Candidate review scope">
          <strong>{showAllCandidates ? "All quarantined tracks" : "Round 1 · eight tracks"}</strong>
          <button type="button" onClick={() => setShowAllCandidates((current) => !current)}>
            {showAllCandidates ? "Show Round 1 only" : "Show held-back tracks"}
          </button>
        </div>
      )}
      <div role="list" aria-label="Available quarantined review candidates">
        {visibleCandidates.map((candidate) => (
          <div key={candidate.review_id} role="listitem" className="quarantined-review-candidate">
            <button
              type="button"
              disabled={disabled || active}
              onClick={() => onStart(candidate.review_id)}
              aria-label={`Start quarantined review Track ${candidate.alias}`}
            >
              Start Track {candidate.alias}
            </button>
            <fieldset aria-label={`Rating for Track ${candidate.alias}`}>
              <legend>Track {candidate.alias} rating</legend>
              {(["good", "distracting", "reject"] as const).map((choice) => (
                <label key={choice}>
                  <input
                    type="radio"
                    name={`rating-${candidate.alias}`}
                    checked={ratings[candidate.alias]?.choice === choice}
                    onChange={() =>
                      updateRating(candidate.alias, choice, ratings[candidate.alias]?.note ?? "")
                    }
                  />
                  {choice}
                </label>
              ))}
              <label>
                Short note (optional, {REVIEW_NOTE_MAX_LENGTH} characters maximum)
                <textarea
                  value={ratings[candidate.alias]?.note ?? ""}
                  maxLength={REVIEW_NOTE_MAX_LENGTH}
                  onChange={(event) => {
                    const current = ratings[candidate.alias];
                    if (current) updateRating(candidate.alias, current.choice, event.target.value);
                  }}
                  disabled={!ratings[candidate.alias]}
                />
              </label>
            </fieldset>
          </div>
        ))}
      </div>
      <button type="button" onClick={() => void exportRatings()}>
        Copy blind-triage JSON summary
      </button>
      <p aria-live="polite">{copyMessage}</p>
      {exportText && (
        <label>
          Selectable blind-triage JSON (not representative-work-session QA, not a second reviewer,
          and not publication or private-beta approval)
          <textarea readOnly value={exportText} aria-label="Selectable blind-triage JSON" />
        </label>
      )}
    </section>
  );
}
