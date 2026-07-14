import { beforeEach, describe, expect, it } from "vitest";
import {
  buildReviewExport,
  loadReviewRatings,
  REVIEW_NOTE_MAX_LENGTH,
  reviewNamespaceForAliases,
  saveReviewRatings,
} from "./reviewRatings";

const aliases = ["E", "F"];

beforeEach(() => window.localStorage.clear());

describe("quarantined review ratings", () => {
  it("persists valid ratings across reloads", () => {
    const namespace = reviewNamespaceForAliases(aliases);
    expect(saveReviewRatings(namespace, aliases, { E: { choice: "good", note: "steady" } })).toBe(
      true,
    );
    expect(loadReviewRatings(namespace, aliases)).toEqual({
      E: { choice: "good", note: "steady" },
    });
  });

  it("isolates ratings by opaque candidate set", () => {
    const deepWork = reviewNamespaceForAliases(["E", "F"]);
    const learning = reviewNamespaceForAliases(["I", "J"]);
    saveReviewRatings(deepWork, ["E", "F"], { E: { choice: "reject", note: "" } });
    expect(loadReviewRatings(learning, ["I", "J"])).toEqual({});
  });

  it("fails safely for malformed, unknown, or oversized stored values", () => {
    const namespace = reviewNamespaceForAliases(aliases);
    const key = `adhd-music.quarantined-review-ratings:${namespace}`;
    window.localStorage.setItem(
      key,
      '{"schemaVersion":1,"namespace":"opaque-aliases:E,F","ratings":{"E":{"choice":"good","note":"ok","extra":true}}}',
    );
    expect(loadReviewRatings(namespace, aliases)).toEqual({});
    window.localStorage.setItem(key, "not json");
    expect(loadReviewRatings(namespace, aliases)).toEqual({});
    expect(
      saveReviewRatings(namespace, aliases, {
        E: { choice: "good", note: "x".repeat(REVIEW_NOTE_MAX_LENGTH + 1) },
      }),
    ).toBe(false);
  });

  it("exports stable aliases with truthful partial-completion and evidence limits", () => {
    const namespace = reviewNamespaceForAliases(["F", "E"]);
    const output = buildReviewExport(
      namespace,
      ["F", "E"],
      { F: { choice: "distracting", note: "busy" } },
      "2026-07-11T00:00:00.000Z",
    );
    const parsed = JSON.parse(output);
    expect(parsed.ratings.map((entry: { review_alias: string }) => entry.review_alias)).toEqual([
      "E",
      "F",
    ]);
    expect(parsed.ratings[0].rating).toBeNull();
    expect(parsed.completion).toEqual({
      candidates_available: 2,
      ratings_recorded: 1,
      all_candidates_rated: false,
    });
    expect(parsed.evidence_limits).toEqual({
      blind_triage_evidence_only: true,
      not_representative_work_session_qa: true,
      not_a_second_reviewer: true,
      not_publication_or_private_beta_approval: true,
    });
    expect(output).toBe(
      buildReviewExport(
        namespace,
        ["E", "F"],
        { F: { choice: "distracting", note: "busy" } },
        "2026-07-11T00:00:00.000Z",
      ),
    );
  });
});
