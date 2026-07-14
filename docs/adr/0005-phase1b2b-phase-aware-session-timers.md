# ADR 0005: Phase 1B2b phase-aware session timers

Date: 2026-07-10
Status: Accepted
Supersedes: none
Superseded by: none

## Context

The earlier interval model multiplied work duration by repeat count and ignored
breaks. Its snapshot exposed only generic elapsed and remaining values, so it
could not truthfully describe interval phase, focus-work time, or final-break
omission. Timer choices were neither configurable in the UI nor durable per
activity. Phase transitions must remain aligned with native CPAL transport and
must not weaken the hardened catalogue/import boundary.

## Decision

- `SessionType` validates bounded countdown and interval values before use.
  Countdown supports 1 minute through 8 hours. Intervals support work periods of
  1 minute through 4 hours, breaks of 1 through 60 minutes, 1 through 12 rounds,
  and at most 12 hours total. Checked arithmetic rejects overflow.
- The session stores a monotonic active timeline. Interval phase, round,
  focus-work elapsed, phase remaining, and total remaining are derived
  deterministically. Breaks occur only between work rounds; expiry is the exact
  end of the final work round. Large time jumps resolve directly to the final
  phase. Paused wall time never accrues.
- Timer configuration and activity changes are allowed only while Idle, Stopped,
  or Expired. Starting after Stop or Expiry resets timing and round state.
- SQLite migration 0003 stores one optional timer configuration per activity in
  explicit constrained columns: kind, countdown seconds, work seconds, break
  seconds, and repeats. It does not use opaque JSON. Activity changes restore
  both intensity and timer configuration.
- Timer changes stage a candidate domain session, write SQLite, and commit the
  domain state only after persistence succeeds. Invalid or failed writes do not
  alter the active configuration.
- The coordinator maps interval Work to native Playing and Break to native
  Paused. It compares the final phase with current audio state, so a tick crossing
  several boundaries performs at most the one transition required by the final
  phase. User pause/resume during an already silent break issues no duplicate
  audio command. Automatic transition failure leaves the prior domain state and
  audio state intact and is returned to the visible error banner.
- React uses native timer radios, compact countdown presets with bounded custom
  minutes, and bounded interval fields. Controls are disabled while active. The
  timer announces phase, round, focus elapsed, current-phase remaining, total
  remaining, and completion.

## Consequences and honest limitations

Infinite sessions count focus time upward. Countdown sessions play continuously
until exact expiry. Interval breaks are smooth native silence in this version;
there is no break track, notification sound, music selection, decoder, or pack
playback. Timer persistence is local per activity and has no cloud or account
sync. The application still uses the same procedural test source for every work
phase and makes no clinical or treatment claim.

The native clock currently has whole-second command resolution, while the UI
polls more frequently for responsiveness. Suspend or delayed polling may cross
multiple boundaries at once; deterministic final-phase resolution avoids
replaying transitions that happened while the process was not ticking.
