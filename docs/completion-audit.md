# Completion audit

Status date: 2026-07-11

This audit compares the original product objective and `docs/product-spec.md`
with authoritative repository and build evidence. A passing build is not treated
as proof of music effectiveness or of untested operating-system behaviour.

## Evidence scale

- **Proven**: direct implementation plus a relevant automated or runtime check.
- **Partial**: implemented, but the full acceptance boundary is not verified.
- **Missing**: required behaviour is not implemented or no adequate evidence exists.
- **Human gate**: automation cannot establish the requirement.

## Original objective

| Requirement | Status | Current evidence | Remaining proof or work |
| --- | --- | --- | --- |
| Five focus activities: Motivation, Deep Work, Creativity, Learning, and Light Work | Proven structurally | Domain activity enum, activity selector, persistence and state-machine tests | Music for four activities remains behind the human gate below |
| User-selectable genre | Proven structurally | Metadata-derived genre availability, exact-match selection, per-activity persistence, UI and tests | Human-accepted catalogue breadth is incomplete |
| User-selectable mood that affects playback | Proven structurally | Metadata-derived playable moods, exact activity/genre/mood intersection, per-activity persistence, UI and tests | Installed-device confirmation is pending |
| Off, Low, Medium, and High/ADHD stimulation | Proven technically | Native DSP profiles, click-smoothing and loudness-compensation tests, inactive preference persistence | This is an adjustable stimulation effect, not a proven ADHD treatment; individual listening validation remains necessary |
| Dynamic, high-quality, non-distracting focus music | Human gate | Immutable generation lineage, decode/analyser gates, blind review applications and local rating export | Blind human ratings and acceptance decisions are still required for Learning, Creativity, Motivation, and Light Work candidates |
| Offline Windows application | Partial | Tauri NSIS/MSI bundles, native CPAL playback, embedded Deep Work private-beta pack, secure pack import | Installed-device playback, suspend/resume, device switching and uninstall checks remain |
| Cross-platform foundation | Partial | Rust core crates, Tauri/React boundary and Windows/Ubuntu/macOS CI definitions | Hosted Ubuntu/macOS runs and native bundle/runtime checks are not yet available |
| No copying or network dependency on any named third-party product | Proven | Locally generated/licensed content pipeline and offline pack architecture | Continue enforcing provenance for every accepted track |

## Version 1 acceptance audit

| Product-spec requirement | Status | Gap |
| --- | --- | --- |
| First-run onboarding | Partial | Version-10 local migration, validated global preferences, transactional coordinator and accessible React flow | Physical listening verification has not been performed; native audio/persistence rollback needs a dedicated command-level test |
| Activity, genre, mood, intensity and timer persist independently | Proven | Covered by persistence and UI/backend tests |
| High changes DSP without changing master volume | Proven | DSP compensation tests cover profile loudness; persisted native master volume is independently controlled and tested |
| Live intensity switching is click-free | Proven technically | Smoothing and active-update tests pass; device listening check remains |
| Starter tracks contain no speech, lyrics, clipping, abrupt silence, or unreviewed licence | Human gate | Mechanical and provenance gates exist; the multi-activity starter catalogue is not human accepted |
| Continuous track and timer transitions | Partial | Authored-loop, crossfade eligibility and timer transition tests pass; installed-device long-session check remains |
| Offline after pack installation | Partial | All normal playback paths are local; installed-device offline check remains |
| Effectiveness and enjoyment rated independently | Proven structurally | Focus effectiveness (`helps focus`, `neutral`, `distracting`, or unset) and enjoyment (`liked`, `not for me`, or unset) use separate local records and UI questions; the local Favorites library is activity-scoped from `liked` only and does not affect focus selection |
| No treatment language | Partial | Product copy is deliberately non-medical; a final rendered-copy audit remains |
| Distraction-free focus view | Proven structurally | React component/App tests cover active-only entry, minimal surface, truthful time labels, pause/resume, Escape-only exit, focus management, and stopped/expired auto-exit | Physical Windows keyboard, contrast, and playback verification remain |
| Focused-window transport shortcuts | Proven structurally | Space toggles only active transport outside editable/native controls; delivered Media Play/Pause and Media Stop events are serialized and tested | Physical Windows media-key delivery remains a focused-window manual verification; no system-wide capture is implemented |
| Windows install/playback/suspend/device-switch/uninstall release matrix | Missing | Installer build exists, but the complete physical Windows matrix has not been recorded |

## Additional Version 1 scope gaps

The following committed scope in `docs/product-spec.md` is not yet complete:

- recent-session history and end-of-session rating;
- signed Windows binaries, which require signing credentials;
- a human-accepted starter catalogue across all five activities and intended
  genres/moods.

## Current release evidence

- Updated Windows NSIS installer:
  `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe`
- NSIS SHA-256:
  `8EB7C744419594A81F34FF45B6838E4CC1F3DFB062B9E98DBD00AC8B97646D23`
- Updated Windows MSI:
  `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi`
- MSI SHA-256:
  `5ED1C00214CB8DA5F1E89F79F8C5F09DB370BE40C65B8CE6133094F397362732`
- Consolidated automated gate result: Rust workspace formatting, Clippy with
  warnings denied, all workspace tests, frontend formatting, lint, type checking,
  98 frontend tests, production build, and Windows NSIS/MSI packaging passed.

The generated WiX source was inspected after bundling and directly lists the
canonical private-beta manifest and its finalized FLAC asset. An earlier
executable-only candidate was superseded after this packaging omission was found.

This evidence supports continued private testing. It does not support declaring
the full product objective complete.

The remaining multi-activity human gate has an eight-candidate first round in
`docs/blind-review-round1.md`, prioritised from the mechanical evidence without
granting approval or rejecting the held-back candidates. Round 1 remains pending
representative-task listening.

This consolidated candidate includes master volume, focus view, focused-window
media transport, installed-track previous/next crossfades, first-run onboarding,
Favorites, and recent-session ratings. It has not been installed over the user's
current app or physically verified.
# Master volume completion audit

Implemented global persisted master volume with a version-9 SQLite migration, native post-DSP bounded gain, Tauri get/set commands, and accessible slider UI. Review candidate identity and export paths are unchanged. Physical-device listening verification has not been performed.
# Installed-track navigation audit

Navigation retains validated predecoded programs and CPAL continuity. It has not been physically listening-tested.

# Favorites library audit

The inactive UI has a collapsed local Favorites library backed only by activity-scoped `liked` enjoyment. It revalidates installed packs before listing or direct start, excludes review content, and starts the exact chosen installed item through the coordinator before committing recent playback. Automated checks cover API/component behaviour and source-level preparation paths; physical-device playback has not been performed.

# Recent-session history audit

Implemented a local SQLite migration and inactive recent-session display with optional independent session ratings. Interrupted rows are reconciled without inventing duration. The consolidated installer contains this change; physical-device verification remains pending.
