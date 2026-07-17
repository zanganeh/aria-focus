# Audio and content design

## Design objective

Audio should be musically credible and slowly dynamic while remaining safe to
ignore. "Dynamic" means controlled evolution over minutes, not surprise. The
ideal session provides enough structure to reduce under-stimulation without
continually pulling conscious attention from the task.

## Content-production pipeline

Version 1 generates or commissions source material offline. AI generation never
runs in the playback path.

1. Define an activity/genre/mood brief.
2. Generate or compose several instrumental candidates.
3. Reject candidates with vocals, obvious hooks, sharp drops, excessive builds,
   unstable tempo, conspicuous solos, or large instrumentation changes.
4. Normalise and analyse surviving candidates.
5. Mark safe loop, continuation, and crossfade regions.
6. Render or configure Off, Low, Medium, and High processing variants.
7. Conduct headphone and speaker QA while performing representative work.
8. Publish only tracks with provenance, licence, measurements, and human approval.

Recommended first providers are Lyria 3 or Eleven Music for high-quality source
generation. Stable Audio or ACE-Step can later provide a local/open-weight path.
Every generated asset must retain its provider terms and generation metadata.

## Generation brief template

Prompts should specify:

- Instrumental only; no voice, speech, vocal samples, chanting, or whispers
- Exact approximate tempo and stable meter
- A restrained melodic range without a lead hook
- Stable instrumentation and textural density
- No drops, breakdowns, dramatic builds, stingers, or sudden silence
- Slow variation through timbre, voicing, and background texture
- A long neutral intro and outro suitable for crossfading
- Activity, genre, and mood requirements

Provider output is a candidate, never an automatically published track.

## Analysis and rejection gates

The offline analyzer calculates at least:

- Integrated loudness and true peak
- Short-term loudness range
- Spectral centroid and high-frequency energy
- Onset density and change over time
- Tempo confidence and drift
- Section-change novelty
- Silence and near-silence regions
- Clipping, discontinuities, and codec errors
- Vocal/speech likelihood

Phase 1C2a implements the codec, loudness, spectral, onset, silence, clipping,
non-finite, and discontinuity measurements as the separate offline
`audio-analyzer` tool. Tempo, structural novelty, and vocal/speech inference are
currently emitted as explicit `not_assessed` values rather than fabricated
probabilities. See `docs/audio-analyzer.md` for the versioned schema, provisional
thresholds, CLI, and exact limitations.

Initial numeric thresholds are calibration values, not scientific constants.
They must be tuned using the starter library and listening tests. Hard failures
include clipping, speech, unexplained silence, corrupt decoding, and licence or
provenance gaps.

## Playback construction

At session start, the pack service revalidates the complete registry and every
installed tree, selects one or two eligible items, and decodes their entire
assets to bounded interleaved `f32` PCM. Symphonia 0.6 decodes WAV PCM, FLAC,
and MP3. Version 1 rejects Ogg Opus rather than accepting content the player
cannot decode. Assets must decode as their declared sample rate, mono/stereo
channel count, bit depth when declared, and duration within 150 ms. The exact
opened handle is hashed before it is rewound and decoded. Encoded files are
capped at 512 MiB and the complete decoded program at 64 Mi samples.

The CPAL control thread resamples each clip once to the selected device rate
with Rubato's synchronous FFT path and maps mono/stereo into the device layout.
It builds the immutable playback program before constructing the callback. The
callback performs no allocation, file access, logging, database work, or UI
callback; it owns only fixed state, immutable PCM, atomics, and prebuilt DSP.

Two-item programs alternate using equal-power crossfades beginning inside both
items' author-marked `crossfade` regions. One-item programs may repeat only
inside that same item's author-marked `loop` region. Overlap is limited by both
authored intervals and eight seconds. A constant program-wide gain calculated
before the callback supplies enough headroom for dry PCM and every actual
overlap without a transition-only level step. If neither construction is
possible, the item is not eligible for continuous mode.

Selection ranks positive activity suitability descending, then pack, item, and
variant IDs for a stable tie-break. It requires approved QA, a playable codec,
mono/stereo audio, a continuous safe region, and all four intensity profiles.
Requiring Off/Low/Medium/High preserves gap-free atomic intensity changes during
an active session. A one-item recent history prevents immediate repetition when
at least two distinct eligible items exist. Callback-published track identity
keeps the displayed source and stop/expiry recent history tied to the item
actually playing. Pause and silent breaks freeze the source after its short
fade-out. The procedural tone is used only as an explicit fallback when no
installed item is eligible.

## Stimulation research implementation

The processing module is deliberately isolated behind this interface:

```text
process(input_pcm, sample_rate, intensity_profile, automation, output_pcm)
```

The initial research processor may use standard, configurable amplitude/tremolo
and noise-mixing techniques with artifact-free parameter smoothing. Off must be
bit-transparent apart from shared gain, resampling, and transition processing.

Profiles must store rate, depth, waveform, wet/dry mix, frequency scope, channel
relationship, ramp time, and output compensation. The UI exposes only intensity.

We must not claim that a generic 16 Hz effect reproduces any named third-party product. Named focus-music products may hold active patents involving selected frequency/cochlear regions, rhythmic-event synchronisation, modulation-feature selection, and personalisation. Public or commercial release of similar DSP requires a claim-level patent review and may require a design-around or licence.

## Evaluation

Audio evaluation has three independent dimensions:

1. Technical quality: no clicks, clipping, dropouts, codec failures, or loudness jumps.
2. Musical quality: coherent, enjoyable enough to sustain use, and not monotonous.
3. Functional suitability: low distraction and subjectively useful during work.

Each candidate is tested on reading, writing/coding, repetitive administration,
and an intentionally boring vigilance task. Ratings capture sound enjoyment,
perceived distraction, task persistence, fatigue, and desire to skip.

The application may offer an optional personal A/B protocol comparing Off,
Medium, and High across matched sessions. Results are personal productivity data,
not diagnosis or clinical evidence.
