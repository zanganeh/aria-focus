# Candidate ledger

`candidate-ledger` records a validated generation candidate without publishing it
or assigning any human-QA, licence-approval, or publication state.

```powershell
cargo run -p candidate-ledger -- validate-plan --plan .\batch.json
cargo run -p candidate-ledger -- register-generated --plan .\batch.json --candidate c-001 --asset .\candidate.flac --analysis .\analysis.json --evidence .\generation-evidence.json --output .\record.json
```

`validate-plan` only snapshots and validates the supplied planned batch.  It
does not write. `register-generated` snapshots all inputs through opened file
handles, verifies the actual FLAC stream and analyzer report, cross-checks the
versioned generation evidence, then atomically creates the generated record.
Inputs are never modified and an existing output is always rejected.

Generated records are intentionally technical provenance only. Their exact
top-level fields are `schema`, `schema_version`, `lifecycle`, `candidate`,
`batch`, `verified`, `evidence`, and `edit_lineage`. `candidate` is a canonical
copy (genre IDs, mood IDs, and inference parameters sorted); the input plan is
never changed. `verified` contains the captured asset name, bytes, SHA-256,
codec, decoded sample rate/channels/frames/duration and captured analyzer name
and SHA-256. `evidence` contains the captured evidence name/SHA-256 plus the
validated `generated_at` (RFC3339 UTC), `machine`, and `gpu` from generation
evidence. The record does not imply listening review, CC0, licence approval,
or publication.

## Deterministic test fixture

`tools/candidate-ledger/tests/fixtures/sine-220hz-10s-48khz-stereo.flac` is
test-only audio, not product music. It is a 10.000-second, 48,000 Hz, stereo
FLAC containing a low-amplitude (0.01) 220 Hz sine. It was generated with
`ffmpeg version 7.1-full_build-www.gyan.dev` using:

```powershell
ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=220:sample_rate=48000:duration=10" -filter:a "volume=0.01" -ac 2 -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a flac -compression_level 5 sine-220hz-10s-48khz-stereo.flac
```

Decoded facts: codec `flac`, `48000` Hz, `2` channels, `480000` frames, and
duration `10.000000` seconds. SHA-256:
`d9ed63c88405ccae2e655d22bc6c0094c73a918045d9e7659e2664513db21b09`.

Decoder-rejection fixtures (test-only) were also generated with FFmpeg
`7.1-full_build-www.gyan.dev`:

```powershell
ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=220:sample_rate=44100:duration=10" -filter:a "volume=0.01" -ac 2 -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a flac -compression_level 5 sine-220hz-10s-44100hz-stereo.flac
ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=220:sample_rate=48000:duration=10" -filter:a "volume=0.01" -ac 1 -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a flac -compression_level 5 sine-220hz-10s-48khz-mono.flac
```

Both are 10.000 seconds of low-level 220 Hz sine. The first is stereo/44,100 Hz
with SHA-256 `fa02960ea2a019feaeae26932eed962bcda2bd4a73dc886aa08e63f189d429ea`;
the second is mono/48,000 Hz with SHA-256
`c2df630c0a28431123386c2d2a47d4fcdf2a8a50533f861d562beca03d5162ca`.

## CLI smoke verification

On 2026-07-11, a temporary directory containing a copy of the committed fixture
was used for these standalone commands: `candidate-ledger validate-plan`,
`audio-analyzer --input asset.flac --output analysis.json`, and
`candidate-ledger register-generated` with evidence constructed from the
captured asset and report facts. Results were respectively exit `0`, `0`, and
`0`; the generated record was created. The temporary directory was removed.
The success smoke uses no device or network.
