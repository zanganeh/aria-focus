# ADR 0003: Phase 1B1 local activity preferences and visible command failures

Date: 2026-07-10
Status: Accepted
Supersedes: none
Superseded by: none

## Context

Phase 1A proved native CPAL playback for one Deep Work session but did not let
users select the five Focus activities, remember a preferred intensity for each
activity, or see native command failures. Phase 1B1 must add those boundaries
without pretending that a content catalogue or activity-specific music exists.

## Decision

- Deep Work, Motivation, Creativity, Learning, and Light Work are independent
  domain activities and native radio choices in React. Their descriptions and
  sound directions are the product definitions in `docs/product-spec.md`.
- Activity changes are rejected by the domain while transport is Playing or
  Paused. The UI disables the selector in those states, while the backend rule
  remains authoritative for stale or non-UI callers.
- Local preferences use a new device-independent `crates/persistence` boundary.
  Production uses exactly pinned `rusqlite 0.32.1` with bundled SQLite for
  desktop portability and explicit numbered SQL migrations.
- The database lives in Tauri's platform app-data directory. It stores only the
  last selected activity and the selected intensity for each activity. There is
  no network, account, cloud, telemetry, or audio-device dependency.
- On startup, the coordinator restores the last activity and that activity's
  intensity. If initialization or stored-value validation fails, commands
  return an actionable error and the UI displays it; the app does not silently
  replace or rewrite suspect data.
- Activity and intensity commands stage a candidate domain state, apply the
  audio intensity, then write SQLite, and commit the domain state only after
  both succeed. An SQLite write failure restores the previous audio intensity.
  Failed audio/domain changes are never persisted.
- Native command rejections are caught by the session controller and exposed in
  a compact dismissible `role=alert` banner. Event handlers do not leave rejected
  promises unhandled.

## Consequences and honest limitations

- Returning users recover the last activity and a separate intensity for each
  activity across app launches.
- Phase 1B1 still plays the same procedural test source for every activity.
  Activity selection changes product context and preferences only; it does not
  yet change musical composition, genre, mood, energy, or instrumentation.
- Catalogue metadata, decoded music assets, activity-specific selection,
  favorites, ratings, timer-choice UI, music generation, and cloud sync remain
  out of scope.
- SQLite portability in this ADR covers desktop targets. Mobile storage and
  lifecycle adapters require a later decision and platform testing.
