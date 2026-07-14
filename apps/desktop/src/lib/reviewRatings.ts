export const REVIEW_RATINGS_SCHEMA_VERSION = 1;
export const REVIEW_NOTE_MAX_LENGTH = 500;

export type ReviewRatingChoice = "good" | "distracting" | "reject";

export interface ReviewRating {
  choice: ReviewRatingChoice;
  note: string;
}

export type ReviewRatings = Record<string, ReviewRating>;

const STORAGE_PREFIX = "adhd-music.quarantined-review-ratings";
const CHOICES = new Set<ReviewRatingChoice>(["good", "distracting", "reject"]);

function stableAliases(aliases: readonly string[]) {
  return [...new Set(aliases)].sort((left, right) => left.localeCompare(right));
}

export function reviewNamespaceForAliases(aliases: readonly string[]) {
  return `opaque-aliases:${stableAliases(aliases).join(",")}`;
}

function storageKey(namespace: string) {
  return `${STORAGE_PREFIX}:${namespace}`;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowed: readonly string[]) {
  return Object.keys(value).every((key) => allowed.includes(key));
}

function parseRatings(
  value: unknown,
  namespace: string,
  aliases: readonly string[],
): ReviewRatings | null {
  if (!isPlainObject(value) || !hasOnlyKeys(value, ["schemaVersion", "namespace", "ratings"])) {
    return null;
  }
  if (value.schemaVersion !== REVIEW_RATINGS_SCHEMA_VERSION || value.namespace !== namespace)
    return null;
  if (!isPlainObject(value.ratings)) return null;

  const allowedAliases = new Set(stableAliases(aliases));
  const ratings: ReviewRatings = {};
  for (const [alias, rating] of Object.entries(value.ratings)) {
    if (
      !allowedAliases.has(alias) ||
      !isPlainObject(rating) ||
      !hasOnlyKeys(rating, ["choice", "note"])
    ) {
      return null;
    }
    if (!CHOICES.has(rating.choice as ReviewRatingChoice) || typeof rating.note !== "string") {
      return null;
    }
    if (rating.note.length > REVIEW_NOTE_MAX_LENGTH) return null;
    ratings[alias] = { choice: rating.choice as ReviewRatingChoice, note: rating.note };
  }
  return ratings;
}

export function loadReviewRatings(namespace: string, aliases: readonly string[]): ReviewRatings {
  try {
    const raw = window.localStorage.getItem(storageKey(namespace));
    if (raw === null) return {};
    return parseRatings(JSON.parse(raw), namespace, aliases) ?? {};
  } catch {
    return {};
  }
}

export function saveReviewRatings(
  namespace: string,
  aliases: readonly string[],
  ratings: ReviewRatings,
) {
  const valid = parseRatings(
    { schemaVersion: REVIEW_RATINGS_SCHEMA_VERSION, namespace, ratings },
    namespace,
    aliases,
  );
  if (valid === null) return false;
  try {
    window.localStorage.setItem(
      storageKey(namespace),
      JSON.stringify({ schemaVersion: REVIEW_RATINGS_SCHEMA_VERSION, namespace, ratings: valid }),
    );
    return true;
  } catch {
    return false;
  }
}

export function buildReviewExport(
  namespace: string,
  aliases: readonly string[],
  ratings: ReviewRatings,
  generatedAt = new Date().toISOString(),
) {
  const orderedAliases = stableAliases(aliases);
  const entries = orderedAliases.map((alias) => ({
    review_alias: alias,
    rating: ratings[alias] ? { choice: ratings[alias].choice, note: ratings[alias].note } : null,
  }));
  const ratingsRecorded = entries.filter((entry) => entry.rating !== null).length;
  return JSON.stringify(
    {
      schema_version: REVIEW_RATINGS_SCHEMA_VERSION,
      review_namespace: namespace,
      generated_at: generatedAt,
      evidence_limits: {
        blind_triage_evidence_only: true,
        not_representative_work_session_qa: true,
        not_a_second_reviewer: true,
        not_publication_or_private_beta_approval: true,
      },
      completion: {
        candidates_available: orderedAliases.length,
        ratings_recorded: ratingsRecorded,
        all_candidates_rated: ratingsRecorded === orderedAliases.length,
      },
      ratings: entries,
    },
    null,
    2,
  );
}
