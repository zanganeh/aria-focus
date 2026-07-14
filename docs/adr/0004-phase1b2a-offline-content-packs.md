# ADR 0004: Phase 1B2a validated offline content packs

Date: 2026-07-10
Status: Accepted
Supersedes: none
Superseded by: none

## Context

Phase 1B1 has five activity preferences but only a procedural test source. The
next bounded slice needs a production metadata contract and secure offline
import without claiming that imported content is playable or clinically useful.
Archives are hostile input and catalogue state spans both files and SQLite.

## Decision

- `crates/catalogue` is device, Tauri, SQLite, and audio-callback independent.
  Serde types define versioned pack, taxonomy, item, variant, provenance,
  analysis, activity suitability, safe-region, stimulation, and QA records.
- Genres and moods are pack data. All five activities are explicit suitability
  records; they are not inferred from genre names.
- Published validation is fail-closed. Named calibration thresholds are ingest
  policy only and carry no medical or neuroscience claim.
- `.adhdpack` is a constrained ZIP containing canonical `manifest.json` and only
  declared `assets/...` files. A bounded ASCII-only cross-platform path grammar
  rejects device names, filesystem aliases, Unicode/case ambiguity, explicit
  directories, links, and traversal. Manifest and asset reads are hard bounded;
  import performs path/type/size/ratio/hash checks in temporary staging before
  an atomic rename.
- The persistent registry uses numbered SQLite migration 0002. It records packs,
  globally unique content items, and pack-owned taxonomy. A synced durable
  receipt records the exact transaction before the pack-directory rename. Pack
  files commit before the database transaction; normal database failure triggers
  rollback. Startup finalizes verified receipt-backed crash orphans, clears stale
  receipts after committed transactions, and preserves actionable evidence for
  missing/corrupt targets or failed recovery. Untracked directories are reported
  without deletion.
- Registry install paths are untrusted data. The canonical registry manifest and
  every related field are validated before deriving the sole content-root target;
  no filesystem operation uses the stored path. Installed roots are closed-world
  directories containing only plain `manifest.json` and `assets` entries.
- On Unix, receipt directory entries and the installed target's parent directory
  are synced around create/remove/rename operations. Windows directory handles
  require different flags, so its explicit durability boundary is a synced
  receipt file plus startup reconciliation.
- Tauri uses the official dialog plugin to select one `.adhdpack`. Native
  commands return safe summaries only, never local install paths. Startup/list
  integrity validation makes damaged files or registry records visible.
- `tools/content-ingest` only hashes, canonicalizes, validates, and packages
  author-supplied local files. Generation, downloading, audio analysis, and
  licence acquisition are outside this tool.

## Consequences and honest limitations

The app can safely register structurally valid offline catalogue metadata while
preserving the Phase 1A native audio and Phase 1B1 preference boundaries. V1
hashes provide integrity but not publisher identity; signatures and trust policy
are deferred. Analysis and QA claims are author-supplied and not independently
recomputed. No reviewed production music ships in this slice, and imported
assets are not connected to a decoder, selector, or audio callback. Patent and
licence review remain mandatory before public or commercial distribution.

The receipt is a local crash-recovery journal, not a cryptographic authorization
record. Corrupt or externally modified receipts, unexpected pack directories,
and incomplete targets deliberately block catalogue startup with an actionable
error; automatic deletion is limited to the exact target created by the current
ordinary-error import attempt.
