# ADR 0001: Phase 0 audio output via the WebView (Web Audio), not native CPAL

Date: 2026-07-10
Status: Superseded
Supersedes: none
Superseded by: ADR 0002

## Context

`docs/architecture.md` specifies a Rust audio engine with a CPAL device backend
on the real-time thread, and the rule "The UI never manipulates raw audio
buffers." Phase 0 must still deliver a working vertical slice that plays
processed audio on Windows, while proving the clean Rust/React boundary.

## Decision

For Phase 0 only:

- The Rust `audio-engine` crate owns all DSP: procedural test-tone generation,
  artifact-smoothed stimulation processing, gain compensation, and WAV encoding.
- The Tauri backend exposes `render_tone(intensity)`, which renders the chosen
  intensity to a PCM16 stereo WAV and returns it as base64.
- The React UI plays the Rust-rendered WAV via the Web Audio API and crossfades
  between intensity renders so switching stays continuous and click-free.

The UI plays opaque blobs produced by Rust; it does not synthesise or transform
audio samples itself. This keeps the DSP boundary intact for Phase 0.

## Consequences

- The WebView audio path, not a native CPAL stream, is the Phase 0 output.
- Real-time, per-block parameter smoothing during live playback is proven by
  deterministic Rust unit tests, not by a live native stream.
- Device switching, OS suspend/resume of the native engine, and the real-time
  output limiter are NOT yet exercised and remain Phase 1 work.

This is an honest limitation, not faked native-engine coverage. Phase 1 replaces
the WebView playback path with the CPAL backend described in the architecture,
reusing the `audio-engine` DSP crate unchanged.
