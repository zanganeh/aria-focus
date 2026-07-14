# Verification plan

## Evidence policy

A requirement is complete only when current code, test output, packaged binaries,
or observed runtime behaviour proves it. Checklists and implementation claims are
not substitutes for evidence.

## Automated gates

- TypeScript formatting, linting, type checking, and unit tests
- Rust formatting, Clippy with warnings denied, unit and integration tests
- SQLite migration tests from an empty and previous-version database
- Catalogue schema, asset-hash, provenance, and licence validation
- Deterministic DSP tests using reference PCM and tolerances
- Transition tests for peak overshoot, discontinuity, and timing
- Decoder tests for corruption, manifest size/rate/channel/bit-depth mismatch,
  finite bounded output, and supported-codec fixtures
- Offline-analyzer tests for deterministic JSON, supported-codec metadata,
  anti-phase-safe loudness/spectral measurements, immutable source snapshots,
  silence, clipping, non-finite PCM, provisional discontinuity candidates,
  atomic no-clobber reports, and structured decode/corruption failures
- Selection tests for activity suitability, QA/profile/safe-region eligibility,
  stable ties, and immediate-repeat avoidance
- Session state-machine tests including pause, stop, expiry, and recovery
- Windows packaging smoke test in CI
- Native Rust workspace compile, Clippy, and headless test coverage in CI on Windows, Ubuntu,
  and macOS; see `adr/0009-native-portability-ci-foundation.md` for the evidence boundary
- Dependency licence and vulnerability reports

## Audio test corpus

Include fixtures for 44.1/48/96 kHz, mono/stereo, 16/24-bit and floating-point
PCM, supported compressed formats, corrupt files, very short tracks, silence,
clipped content, and deliberately discontinuous loop points.

Tests must prove:

- Off processing is transparent within documented tolerances
- Parameter changes are smoothed and click-free
- Low, Medium, and High produce measurably distinct modulation depths
- Output gain compensation prevents intensity from masquerading as loudness
- Crossfades have no zero gap, clipping, or channel swap
- Mono/stereo sources survive one-time device-rate conversion and device-layout mapping
- Timer transitions do not block or starve the audio callback

## Windows runtime matrix

Test at minimum:

- Windows 11 current stable, x64
- Built-in speakers, wired headphones, Bluetooth headphones, and HDMI where available
- Default-device removal and replacement during playback
- Sleep, wake, screen lock, and user session switch
- Media keys and competing audio applications
- With the app window focused, verify physical Windows delivery of Media Play/Pause
  and Media Stop, including when a native control has focus. Automated tests cover
  delivered KeyboardEvents only; they do not establish system-wide capture.
- Offline launch and playback
- Install, upgrade preserving data, and uninstall
- Long sessions of 2, 4, and 8 hours with memory/CPU/dropout monitoring

## Cross-platform CI boundary

The native-portability CI matrix is configured to check and test the full Rust workspace on Ubuntu
and macOS, while the Windows Rust job provides the same coverage on Windows. A successful matrix
run proves host-native compile and headless test compatibility only. It does not prove audio-device
runtime behaviour, Tauri UI smoke behaviour, signing/notarization, or Linux/macOS bundle/install
flows. Windows remains the only CI packaging target.

## Startup recovery checks

Automated desktop tests cover independent core and content-pack health, successful and partial
retry, retained healthy service identity, repeated and concurrent compare-before-commit retries,
and app-data-path failure. These are non-destructive recovery checks: neither retry nor its tests
delete, rename, reset, or recreate user databases or installed content. A real Windows startup
smoke still needs an explicitly chosen disposable app-data location before it may be recorded.

Every app or bundled-library release must also follow
[`content-pack-upgrades.md`](content-pack-upgrades.md). In particular, the Windows upgrade matrix
must include a database containing both the retired v1 pack and current v2 pack; clean-install
coverage cannot detect retirement-order regressions.

## Product checks

- One-click return to the last-used session
- Independent persistence for activity, sound, intensity, and timer
- Keyboard-only operation and visible focus state
- Screen-reader names for controls and non-colour intensity indicators
- Reduced-motion support and a genuinely distraction-free player
- User data export and deletion
- No medical claims in application copy, metadata, installer, or website material

## Focus view verification

Automated React tests verify entry from an active session, the deliberately
minimal rendered surface, initial focus on Pause/Resume, native button actions,
Escape exiting without pausing playback, focus restoration to the entry control,
truthful infinite/countdown/interval time labels, and automatic exit when the
session becomes stopped or expired. This is UI evidence only. It does not verify
physical-device keyboard behaviour, display contrast, or playback behaviour on
an installed Windows build; those remain manual runtime checks.

## Local feedback persistence

Automated coverage must verify the versioned SQLite upgrade preserves installed
packs and focus feedback, stores enjoyment in its own activity-scoped table,
rejects corrupt enum values visibly, and cascades both axes when a pack is
removed. Service/API/UI coverage must also verify an in-flight response cannot
be applied to a newly displayed item, and that enjoyment is absent from focus
selection inputs. These checks validate software behavior only; they do not
constitute human audio review or a claim of medical benefit.

## Human audio review

Each starter track requires two documented reviews, including one review during a
representative work session. Reviewers score technical defects, distraction,
fatigue, musical quality, transition suitability, and each proposed activity.
Any speech, attention-grabbing hook, sharp transition, or unresolved provenance
issue is a release blocker.

## Release evidence

The final verification report links every product acceptance criterion to:

- exact source or configuration implementing it
- automated test and latest passing output where applicable
- packaged build hash
- manual runtime result with OS/audio-device details
- unresolved limitations and patent/licensing status

The application is not complete merely because tests pass; the starter content,
audio quality, Windows package, offline behaviour, and requirement-by-requirement
audit must all be present and verified.
# Master volume verification

Automated coverage verifies value bounds/default, SQLite migration and corrupt-value failure, atomic realtime endpoint behavior and smoothing, coordinator independence, and accessible UI command handling. This does not include physical-device listening verification.
# Installed-track navigation verification

Automated verification covers bounded renderer output, authored transitions, and transport/UI command behavior. No physical listening verification is claimed.
