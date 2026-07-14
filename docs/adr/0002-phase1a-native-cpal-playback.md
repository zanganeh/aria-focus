# ADR 0002: Phase 1A native playback through a Rust CPAL facade

Date: 2026-07-10
Status: Accepted
Supersedes: ADR 0001
Superseded by: none

## Context

ADR 0001 allowed the Phase-0 WebView to request base64 WAV renders and play
them with Web Audio. That temporary path violated the final architecture rule
that the UI never handles audio buffers and could not prove native output
ownership or transactional coordination with the domain session.

CPAL streams are intentionally not `Send` across all supported platforms. A
stream therefore cannot be stored directly in Tauri managed state, which must
be thread-safe.

## Decision

Phase 1A uses these boundaries:

- React sends only session and intensity commands and receives snapshots and
  provenance. It never requests, decodes, renders, or plays PCM/WAV/base64.
- A Rust `AudioFacade` owns playback state. The production implementation uses
  the default CPAL output device.
- A dedicated native control thread creates, owns, starts, and drops the CPAL
  stream. Tauri state holds only its bounded command sender and atomic callback
  controls, preserving CPAL's cross-platform ownership contract.
- The CPAL data callback owns a prebuilt procedural source, DSP processor, and
  transport-gain smoother. It performs no allocation, logging, locks, file or
  network access, database work, or UI callback.
- Off, Low, Medium, and High remain separate DSP profiles. Intensity is sent as
  an atomic fixed-size value; the processor smooths depth, mix, and output
  compensation. Transport pause/resume/stop use a short gain ramp while the
  stream remains alive and writes silence.
- The adapter converts to practical WASAPI/default formats: `f32`, `i16`,
  `u16`, `i8`, `i32`, `u8`, and `u32`. Unsupported formats fail before domain
  state is committed.
- Tauri commands coordinate a cloneable domain session and the audio facade as
  one transaction. If start, pause, resume, stop, expiry-stop, or intensity
  application fails, the previous domain state is restored and the command
  returns an error.
- Device-free mocks test transport transitions and rollback. DSP and renderer
  tests deterministically verify smoothing, intensity separation, and output
  compensation without requiring CI audio hardware.

## Consequences and honest limitations

- The Web Audio `TonePlayer`, `render_tone` command, base64 dependency, and WAV
  render path are removed.
- Native playback now follows the architecture and is reusable through CPAL on
  Windows, macOS, and Linux. Mobile still requires later platform integration.
- Phase 1A uses the licence-clean procedural Deep Work test source. A reviewed
  music catalogue, decoding, resampling, gapless queueing, and crossfading are
  still absent.
- The default output device is selected when playback first starts. Runtime
  device switching, unplug recovery, stream rebuild after a device error, and
  suspend/resume recovery remain unimplemented and must be release-tested.
- Stream errors are recorded atomically and reported on the next control
  command. There is no automatic recovery in Phase 1A.
- Pause and stop keep the native stream open and smoothly silent until the app
  exits. Releasing the device while stopped is deferred until it can preserve
  click-free restart and robust device recovery.
