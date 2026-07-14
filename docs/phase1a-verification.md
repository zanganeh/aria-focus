# Phase 1A native-audio verification report

Date: 10 July 2026

Status: Native Windows audio boundary verified; full Focus MVP not complete.

## Independently verified outcome

- React no longer requests, decodes, buffers, or plays audio. Residue checks
  found no application references to `TonePlayer`, `AudioContext`,
  `render_tone`, base64 audio, WAV encoding, or buffer sources.
- A dedicated Rust control thread owns the non-`Send` CPAL stream. The data
  callback receives a prebuilt procedural source and DSP renderer and uses
  fixed-size atomic controls without allocation, locks, logging, or I/O.
- Tauri coordinates domain and audio state with rollback coverage for start,
  pause, intensity, and countdown-expiry failures.
- The Windows release executable opened the default CPAL output stream and
  completed start, live High / ADHD selection, pause, resume, stop, and restart
  without a visible error or crash. The timer froze while paused and restarted
  from zero after stop.
- Windows UI Automation continued to expose all four intensities as named radio
  controls.

## Review defect found and corrected

Independent review found that the DSP one-pole smoother initially used
`exp(-1/tc)` as the update coefficient even though its recurrence requires
`1 - exp(-1/tc)`. That implementation would have applied almost the entire
nominal 300 ms change in one sample.

The coefficient now uses the numerically stable equivalent
`-exp_m1(-1/tc)`. A deterministic regression test verifies limited first-sample
movement, approximately 63.2% response after one time constant, monotonic
convergence without overshoot, and an Off-to-High transition away from a
carrier zero crossing.

## Reproduced automated evidence

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm verify
pnpm tauri build
```

Results:

- Rust: 28 tests passed (14 audio engine, 9 domain, 5 coordinator); no failures.
- Frontend: 17 tests passed across 3 files; no failures.
- Cargo check, formatting, strict Clippy, Prettier, ESLint, and TypeScript: clean.
- Vite production build: 39 modules transformed successfully.
- Tauri release build: executable, MSI, and NSIS bundles produced successfully.

## Current Windows artifacts

The hashes below are the historical Phase 1A native-audio build. Phase 1B1
later replaced the same output paths; see `phase1b1-verification.md` for the
current activity/persistence build.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/release/adhd-music-desktop.exe` | 4,366,336 | `0D91DCDFC0EA297589D7C564041A71EABC108E8A9895AB80339292CDB2C3F38C` |
| `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi` | 2,043,904 | `FCE3836EE0E57AA550EA5B1357CB15949FA34E7D21A71E807595C31A5ED39CB2` |
| `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe` | 1,388,299 | `547D45B9B7264D02A837B58083EC3480B2DFE433A3D718980ABB67F632D58845` |

The packages are not code-signed and are not asserted to be byte-for-byte
reproducible.

## Remaining limitations

- The source is still a procedural test pad, not a reviewed music catalogue.
- There is no decoder, resampler, gapless queue, equal-power catalogue
  crossfade, output limiter, or content-pack pipeline yet.
- The native stream stays open and silently active after pause or stop. Device
  release and robust click-free reopening remain future work.
- Device switching, unplug recovery, stream-error recovery, sleep/wake, and
  multi-device testing remain outstanding.
- Audible quality was not objectively measured, and this run did not prove
  output across every enumerated audio device.
- Native command errors currently reject to the frontend without a dedicated
  user-facing error banner; this should be addressed before broader testing.
- No medical efficacy is claimed. Patent and legal review remains required
  before public or commercial distribution of stimulation DSP.
