# ADR 0006: Phase 1C1 installed content playback

- Status: Accepted
- Date: 2026-07-10

## Context

Phase 1B2a installed hostile-input-safe content packs but deliberately did not
play their assets. Native Phase 1A playback generated a procedural source inside
the callback. Phase 1C1 needs installed music without weakening pack integrity,
blocking the callback, creating gaps during intensity/timer changes, or implying
that metadata validation establishes clinical effectiveness.

## Decision

The pack service repeats its complete registry and installed-tree verification
on each session start. A pure catalogue selector considers only that validation
pass. It requires positive suitability for the current activity, approved QA,
mono/stereo WAV/FLAC/MP3, an authored loop or crossfade region, and all four
Off/Low/Medium/High availability entries. Ranking is suitability descending,
then pack/item/variant ID. The last successfully started item is skipped when a
second distinct eligible item exists.

Symphonia 0.6 decodes selected files completely to bounded interleaved `f32`
outside the callback. The exact opened file handle is size-checked, hashed,
rewound, and decoded, closing the verification/reopen substitution window. WAV
accepts only declared 16/24/32-bit integer or 32-bit float PCM; FLAC and MP3 are
matched exactly. Sample rate, channel count, declared bit depth, finite samples,
channel alignment, and duration (150 ms tolerance for codec padding) are
checked. Each encoded file is limited to 512 MiB and the complete decoded
one/two-track program to 64 Mi samples.
Format-v1 publication rejects Ogg Opus because this pure-Rust decoder selection
does not support it.

Once CPAL discovers the default device, Rubato 4 performs a single synchronous
FFT whole-clip conversion to its sample rate on the CPAL control thread. Mono
and stereo are mapped into the fixed device layout there. One loop-safe track or
two alternating crossfade-safe tracks are handed to the callback as immutable
PCM plus bounded cursor state. Every overlap begins inside both the outgoing and
incoming authored regions and is limited by both region lengths and eight
seconds. A constant whole-program gain, computed from dry PCM and every actual
equal-power overlap, supplies transition headroom without a transition-only
gain step or hard-clipping the overlap. Loop regions are used only for
repetition of the same item.

The callback publishes only a fixed-size atomic track index. The command/UI side
uses that index to report the track actually playing and polls it while active.
Pause and silent interval breaks advance only through the short fade-out, then
freeze the source at zero gain until resume. User stop and automatic expiry
commit the actual current item to the process-local recent history.

The start command holds pack-service then coordinator locks through validation,
decode, native preparation, and start. Domain state and recent history commit
only after native start succeeds. If no item is eligible, `TestTone` is selected
and labelled as an explicit fallback. Active intensity changes remain atomic
DSP parameter changes; they neither reselect content nor reset timer state.

## Consequences

- Whole-file preparation can still peak near 768 MiB of PCM vectors at the hard
  limit (decoded program, resampler output, and mapped device program), plus
  decoder/resampler overhead. Aggregate checked limits fail before callback
  construction; starter tracks should stay far below the ceiling.
- A source or device change rebuilds the stream only at a stopped-session start.
- The recent-item window is process-local and currently one item deep.
- Multichannel and Ogg Opus packs require a future format/playback revision.
- Symphonia is MPL-2.0. It is used unmodified; MPL obligations apply to its own
  covered source files, not this project's separate files. Distribution must
  retain its notices/source availability as required. Rubato is MIT OR
  Apache-2.0. A release still requires the repository's dependency-licence gate.
- Technical playability and author-supplied QA/provenance are not evidence of
  medical benefit, ADHD treatment, or independent publisher verification.
