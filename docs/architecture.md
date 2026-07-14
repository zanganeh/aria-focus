# Architecture

## Decision

Use Tauri 2 with a React/TypeScript interface and a Rust application/audio core.
The first packaged target is Windows. Platform boundaries must remain compatible
with macOS and Linux desktop targets and with later Android/iOS native adapters.

## Components

```text
React UI
  ├── activity and catalogue browser
  ├── player and timers
  ├── onboarding and preferences
  └── local session history
          │ typed Tauri commands/events
Rust application core
  ├── session state machine
  ├── catalogue and recommendation service
  ├── timer service
  ├── SQLite repositories
  └── audio facade
          │ bounded lock-free messages
Rust audio engine
  ├── decoder and resampler
  ├── gapless queue and crossfader
  ├── stimulation processor
  ├── loudness/output limiter
  └── CPAL device backend
```

## Architectural rules

- The UI never manipulates raw audio buffers.
- The audio callback never waits on the UI, database, network, or filesystem.
- Content selection and DSP configuration are independent.
- All catalogue taxonomies are data-driven.
- Platform-specific behaviour is implemented behind interfaces for media keys,
  power management, audio focus, secure storage, and notifications.
- Network content providers are optional adapters; offline playback is the core.
- Session state is recoverable after a crash or OS suspend without fabricating
  elapsed focus time.

## Installed playback start transaction

```text
start command
  -> lock pack service, then session coordinator
  -> revalidate registry rows and every installed pack tree
  -> deterministically select a loop-safe item or crossfade-safe pair
  -> hash and decode the same bounded open file handles outside the callback
  -> CPAL control thread discovers device, resamples/maps, builds callback
  -> start native stream
  -> commit domain start; stop/expiry records the atomic current-track identity
```

Any validation, decode, resample, device, or stream-start error is returned to
the UI. Domain state and selection history remain unchanged. Timer work/break
transitions continue to use pause/resume atomics and never decode or replace a
source. The source advances through the short fade-out, freezes for the silent
break, and resumes from that cursor without changing elapsed focus time.

## Suggested workspace

```text
apps/desktop/              Tauri application and React UI
crates/domain/             activities, profiles, sessions, ratings
crates/catalogue/          metadata, selection, packs, provenance
crates/audio-engine/       decoding, buffering, transitions, DSP
crates/persistence/        SQLite migrations and repositories
crates/platform/           platform traits and desktop implementations
tools/content-ingest/      local hashing, validation, and pack building
content/starter/           licensed metadata; binary audio ignored or LFS-managed
docs/                      product, audio, architecture, and verification
```

## Stored domain records

- `ContentItem`: immutable ID, asset hash, provenance, licence, analysis, tags
- `ContentVariant`: codec/location plus Off/Low/Medium/High processing metadata
- `UserPreference`: activity defaults, genre/mood weights, sensitivity, volume
- `Session`: chosen profile, monotonic start/end, pauses, tracks, completion
- `Rating`: effectiveness, enjoyment, distraction, fatigue, optional note
- `ContentPack`: manifest, version, hashes, compatibility range; signatures are deferred

## Delivery phases

### Phase 0: toolchain and vertical slice

Create the workspace, Windows Tauri shell, CI, one test asset, one activity, one
intensity processor, continuous playback, and a timer. Prove device switching,
suspend/resume, installer output, and clean architecture before expanding UI.

### Phase 1: Focus MVP

Implement all five activities, metadata filtering, four intensity levels, three
timer types, favorites/ratings, local personalisation, distraction-free player,
content packs, and the reviewed starter catalogue.

### Phase 2: quality and portability

Add macOS/Linux CI builds, mobile platform adapters, richer ingest analysis,
personal A/B testing, signed packs, accessibility polishing, and crash telemetry
that is disabled by default until consent is obtained.

### Phase 3: optional generation

Add provider adapters to a separate authoring tool, never to the real-time audio
thread. Generated tracks still pass automated analysis, provenance checks, and
human QA before becoming playable content.

## Agent handoff

The implementation agent must work from these documents and record deviations as
ADRs. The planning/verification agent independently checks the resulting state;
it must not accept implementation notes as proof. The orchestration environment
must record whether GLM-5.2 and GPT-5.6 Sol were actually selected.
# Master volume

Master volume is a global `MasterVolume` preference (0–100%, default 70%), stored separately from per-activity stimulation intensity. The native realtime renderer applies its bounded linear gain after DSP, independently smooths non-zero changes, and returns exact silence at zero without callback allocation, locks, I/O, or blocking.
# Installed-track navigation

Installed programs expose directional Previous/Next only while playing and only when they contain more than one validated track. The control thread publishes a bounded `u8` target plus a generation; `255` is an explicit invalid sentinel, and the callback validates the target before indexing. The callback owns the manual equal-power crossfade state, using prevalidated authored incoming regions and immutable decoded PCM. It performs no I/O, allocation, locks, logging, decoding, or stream rebuild. A request during the short manual crossfade is rejected until completion; stop clears any pending request and paused transport rejects navigation. The current-source label is read from the renderer-committed atomic track, so it changes only after the crossfade commits.
