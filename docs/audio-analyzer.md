# Offline candidate audio analyzer

Status: Phase 1C2a technical-analysis foundation. Thresholds are provisional
calibration values; this tool does not publish content or replace listening.

## Purpose and boundary

`audio-analyzer` copies one local WAV PCM, FLAC, or MP3 candidate into a bounded
immutable temporary snapshot while hashing it, decodes only that snapshot, and
emits a deterministic, versioned JSON report. It does not modify audio, infer
licensing or provenance, mark safe transition regions, or assign human QA.
Encoded input is capped at 512 MiB and decoded interleaved PCM at 64 Mi samples.

The report contains:

- SHA-256, byte length, detected codec, sample rate, channel count, bit depth,
  decoded frame count, and duration;
- EBU R128 integrated loudness and true peak, plus EBU Tech 3342 loudness
  range when the programme is long enough;
- an explicitly named ungated 400 ms channel-aggregated power percentile-range
  approximation, kept separate from EBU loudness range;
- independently transformed, channel-summed Hann-STFT power for spectral
  centroid, energy ratio at or above 8 kHz, and normalized spectral-flux onset
  density, so opposite-polarity stereo cannot cancel;
- near-silence totals and regions, full-scale sample count, non-finite sample
  count, and inter-sample discontinuity candidates; and
- ordered hard-rejection and calibration-flag reasons with stable codes.

Tempo/BPM, tempo confidence and drift, section novelty, and vocal/speech
likelihood are explicitly `not_assessed` in schema version 1. Spectral
heuristics are not presented as speech detection.

## Usage

From the repository root:

```powershell
cargo run -p audio-analyzer -- --input .\candidates\deep-work-01.flac
cargo run -p audio-analyzer -- --input .\candidates\deep-work-01.flac --output .\reports\deep-work-01.json
```

JSON is written to standard output unless `--output` is supplied. Output files
are fully written and synced in the destination directory before an atomic
no-clobber publish; existing paths are never overwritten. The tool exits `0`
when no hard rejection is present, `1` after emitting a report containing one
or more hard rejections, and `2` for argument, serialization, or report-write
errors.

The same analyzer version, source bytes, input filename, and platform produce
the same field order and values rounded to six decimal places. Keep the report,
analyzer version, and source hash together in the immutable candidate ledger.

## Provisional decision rules

Hard rejection is emitted for decode/corruption failure, non-finite PCM,
samples at or beyond full scale, or an unexplained near-silent run of at least
one second. Adjacent same-channel sample jumps of at least `0.75` produce one
provisional `discontinuity_candidates_require_review` flag regardless of count.
A raw jump is not proof of an audible click; listening or waveform inspection
must confirm a discontinuity before the final content gate rejects it.

Flags currently identify integrated loudness outside `-28..=-14 LUFS`, true
peak above `-1 dBTP`, loudness range above `8 LU`, high-frequency energy ratio
above `0.25`, and onset density above `4 events/second`. These are deliberately
visible calibration values, not clinical or universal music-quality limits.

## Known limitations

- EBU loudness range needs sufficient programme history; short clips can report
  it as unavailable while the separately labelled approximation remains.
- The click detector is a conservative raw adjacent-sample jump detector. It
  does not model audibility or inspect authored loop/crossfade boundaries.
- The onset detector is deterministic spectral flux, not a trained musical
  transcription model.
- No speech/singing model, BPM tracker, tempo-drift estimator, or structural
  segmentation model is included yet.
- Analysis is bounded but whole-file: worst-case memory includes the on-disk
  encoded snapshot, decoder state, up to 256 MiB of decoded `f32` PCM, per-channel
  FFT buffers, and loudness-meter history. It is not a streaming batch service.
- Discontinuity timestamps are capped at the first 32 candidates; the complete
  count and a truncation indicator remain in the report.
- Licence evidence, provenance completeness, notification-like salience,
  musical distraction, safe regions, and representative-session suitability
  remain separate automated or human gates.
