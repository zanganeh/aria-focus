# Phase 1C2a offline analyzer verification report

Date: 11 July 2026

Status: deterministic technical candidate analysis independently verified;
tempo, structure, speech/vocals, provenance, safe regions, and human acceptance
remain separate incomplete gates.

## Verified outcome

- `audio-analyzer` accepts local WAV PCM, FLAC, and MP3 candidates and emits a
  versioned JSON report without modifying source audio or assigning human QA.
- Encoded input is capped at 512 MiB and decoded interleaved PCM at 64 Mi
  samples. Codec, rate, channel layout, bit depth, decoded frames, duration,
  byte length, and SHA-256 are reported from the actual analyzed snapshot.
- Source bytes are copied into a bounded temporary snapshot while hashing. The
  decoder reads only that snapshot, so later changes to the live source cannot
  alter the audio identified by the report hash.
- Loudness uses EBU R128 integrated loudness, true peak, and EBU Tech 3342 LRA
  where sufficient programme history exists. A separately labelled 400 ms
  channel-power percentile approximation remains available for short material.
- Independent per-channel STFT power is summed for centroid, high-frequency
  ratio, and spectral-flux onset measurements. Anti-phase stereo cannot cancel
  into false silence.
- Decode/corruption failure, non-finite PCM, full-scale samples, and a
  provisional one-second near-silence gate are hard rejections. Raw
  adjacent-sample jumps are evidence requiring review, not automatic proof of
  an audible click, and therefore produce one provisional flag.
- Optional output reports are fully written and synced before atomic
  no-clobber publication. Existing report files are preserved.
- Tempo/BPM, tempo confidence/drift, section novelty, and vocal/speech
  likelihood are explicit `not_assessed` values rather than fabricated
  probabilities.

## Independent review corrections

GLM-5.2 implemented the analyzer. GPT-5.6 Sol reviewed the code and found four
issues that were corrected before acceptance:

1. Stereo was averaged before analysis, allowing anti-phase content to cancel
   and corrupt RMS/spectral/onset results.
2. A raw sample jump documented as only a click candidate was nevertheless a
   hard rejection.
3. Hashing an inspected length and then decoding the mutable live handle did
   not guarantee that the hash identified exactly the decoded bytes.
4. Direct report writes could leave a partial file if a write or sync failed.

Regression tests now cover anti-phase stereo energy, live-source append
exclusion, discontinuity classification, and atomic no-overwrite publication.

## Reproduced evidence

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All gates passed. Rust total: 115 tests—31 desktop/coordinator/pack service, 9
analyzer, 32 audio-engine unit, 8 decoder integration, 16 catalogue/import, 2
content ingest, 10 domain, and 7 persistence.

Independent CLI smoke results:

| Input | Exit | Decode status | Evidence |
| --- | ---: | --- | --- |
| committed PCM16 mono 44.1 kHz WAV | 0 | `decoded` | exact codec/rate/channels/hash, zero hard rejections |
| non-audio bytes with `.wav` extension | 1 | `failed` | snapshot hash plus `probe_or_corruption_failure` hard rejection |

Both reports were atomically written to unique temporary paths, parsed as JSON,
and removed after verification.

## Limitations and next gate

- Thresholds are provisional and have not been calibrated against accepted and
  rejected full-length focus music.
- Whole-file analysis is bounded but may use substantial temporary disk and
  memory for maximum-size inputs.
- Raw discontinuity and spectral-flux measurements are not psychoacoustic click
  detection or musical transcription.
- There is no validated speech/singing classifier, tempo tracker, tempo-drift
  estimator, or structural segmentation model yet.
- A successful analyzer exit does not prove licence/provenance, musical
  quality, low distraction, notification-like salience, safe transitions, or
  representative-session suitability.
- No listener-grade candidate or reviewed starter pack exists yet. Phase 1C2b
  must pin the generator and establish an immutable candidate ledger before
  generation begins.

