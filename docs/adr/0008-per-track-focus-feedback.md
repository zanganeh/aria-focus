# ADR 0008: per-track, per-activity focus feedback

## Decision

Installed tracks expose three explicit choices: `Helps focus`, `Neutral`, and
`Distracting`. The choice is scoped to an activity, so one item can have a
different result for Deep Work and Light Work. The procedural test tone has no
feedback control.

Feedback is stored locally in versioned SQLite migration 0005, keyed by
`(item_id, activity)`. `item_id` is a foreign key to `installed_items` with
`ON DELETE CASCADE`; values are constrained and validated on read and write.
The pack service revalidates the installed catalogue before every feedback
read/write and before loading selection feedback.

## Selection

Selection remains pure: the caller supplies only the current activity's map.
Human QA, codec/sample limits, positive activity suitability, and exact genre
matching are hard eligibility checks. `Distracting` is then ineligible both as
a primary and as a crossfade partner. `Helps focus` ranks before neutral and
unrated items; neutral and unrated intentionally share a rank. Existing recent
item avoidance is applied after eligibility, so an eligible distinct item is
used when available. Stable identifiers remain the tie-breakers.

If a concrete genre has no candidate because its items are distracting, the
backend returns an actionable error. It never swaps genre or uses the test
tone. The test tone remains available only when no installed content exists.

## Verification and limitations

Automated tests cover migration versioning/idempotence, overwrite, activity
isolation, restart survival, unknown IDs, corrupt stored values and cascade
removal; selection coverage includes helpful priority, all-distracting no
match, genre/activity eligibility, and stable recent-item exploration. Frontend
tests cover authoritative loading and save behavior.

This feature records personal preferences for local playback only. It makes no
claim to diagnose, treat, or improve ADHD. It does not infer mood, share data,
or explain a scoring algorithm in the primary UI.
