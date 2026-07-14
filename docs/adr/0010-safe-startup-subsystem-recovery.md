# ADR 0010: Safe startup subsystem recovery

## Decision

The desktop app reports session/audio-core and content-pack health separately and offers an explicit retry. Retry re-resolves the app-data directory from the current app handle, then runs the existing core restore and pack integrity/migration constructors only for failed slots.

Candidates are built outside subsystem locks and compare-before-commit prevents a concurrent retry from replacing a service that has already recovered. Healthy session and pack instances are retained unchanged. App-data resolution failure updates only failed slots with the new actionable error.

## Safety boundary

This is recovery from transient startup conditions, not repair. It never deletes, renames, resets, or recreates databases or content. Corrupt preferences or content still require explicit manual, user-authorized handling. Existing playback selection remains unchanged: `start_session` requires the pack service, so a healthy core alone does not broaden procedural fallback behavior.

## Verification

Focused backend tests inject simple constructors, so they cover full, partial, repeated, compare-before-commit, and path-failure recovery without audio hardware. Frontend tests cover healthy-panel suppression and partial-retry visibility.
