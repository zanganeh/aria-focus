# Local ACE-Step production bootstrap

`bootstrap.ps1` creates an ignored local ACE-Step checkout and uv-managed
Python 3.12 environment, validates the pinned source and evidence hashes, and
downloads immutable Hugging Face snapshots. `production.py` validates the
candidate ledger, maps every supported field to the pinned ACE-Step TOML API,
generates candidates one at a time, then records analyzer and ledger evidence.

All runtime data is rooted at `.local/music-generation`; it is deliberately
ignored and never becomes application content. Run:

```powershell
pwsh -ExecutionPolicy Bypass -File tools/music-generation/bootstrap.ps1
pwsh -ExecutionPolicy Bypass -File tools/music-generation/bootstrap.ps1 -DownloadModels
pwsh -ExecutionPolicy Bypass -File tools/music-generation/run-batch.ps1
```

Select the immutable plan explicitly for every direct wrapper action. `run`
uses the selected plan's `batch.id` as its directory unless `--run-id` is a
safe explicit retry identifier; `process` always requires `--run-id`.

```powershell
$root = (Resolve-Path .).Path
$plan = Join-Path $root 'content/plans/deep-work-calibration-v1.json'
python tools/music-generation/production.py preflight --root $root --plan $plan
python tools/music-generation/production.py download --root $root --plan $plan
python tools/music-generation/production.py run --root $root --plan $plan
python tools/music-generation/production.py process --root $root --plan $plan --run-id deep-work-calibration-v1-retry-2
```

The wrapper snapshots a regular, non-link plan at start, hashes it once, sends
that exact path to candidate-ledger, and creates `plan-identity.json` beside a
new run. Every resume/process verifies the plan hash, batch id, selected path,
and run id before reading candidates or artifacts. The existing completed v1
retry directories without that marker are accepted only by `process` as a
read-only legacy verification path when the selected canonical v1 plan and a
`deep-work-calibration-v1-retry-*` run id match; it never writes a marker.

The runner refuses unknown ledger parameters, changed pins, existing output
paths, hard analyzer rejection, and non-48 kHz stereo FLAC evidence. It does
not perform human acceptance.

## Ogg Opus candidate distribution

`convert_library_to_opus.py` makes a new, closed-world candidate pack from a
closed-world FLAC-master pack. It never modifies the masters or an existing
destination. It emits Ogg Opus at fixed 48 kHz stereo, 112 kbps VBR using
`ffmpeg`'s `libopus` encoder, updates each existing item ID and manifest asset
hash/size/codec/path, validates each emitted file using `ffprobe`, and enforces
a 300 MB default package budget. `ffmpeg` and `ffprobe` are required locally.

First validate the full-library paths without writing anything:

```powershell
python tools/music-generation/convert_library_to_opus.py --source apps/desktop/src-tauri/private-beta-pack --output .local/opus-library-candidate --pack-version 0.22.0-opus.1 --app-version-requirement '>=0.22.0, <0.23.0' --dry-run
```

Then generate the staged candidate and its adjacent audit report. The source
pack is only read; the new directory must not already exist.

```powershell
python tools/music-generation/convert_library_to_opus.py --source apps/desktop/src-tauri/private-beta-pack --output .local/opus-library-candidate --pack-version 0.22.0-opus.1 --app-version-requirement '>=0.22.0, <0.23.0' --max-total-bytes 300000000
```

The resulting `manifest.json` is regenerated deterministically from the source
manifest and conversion results. `opus-library-candidate.conversion-report.json`
records source and output hashes plus the exact local encoder/prober versions.

## One internal end-to-end command

`music_pipeline.py` is the supported internal orchestration layer. It composes
the pinned generator and the converter above; it does not reimplement them. The
stages are intentionally explicit:

- `preflight` verifies the plan, local model snapshots, FFmpeg, and ffprobe;
- `generate` creates FLAC masters and immediately runs analysis and candidate-ledger evidence;
- `package` builds a draft, closed-world FLAC candidate pack from one completed run, then creates a separate 112 kbps Ogg Opus pack;
- `all` performs `generate` followed by `package`.

It never downloads models, approves tracks, publishes content, overwrites an
output, writes into a generation run, or writes into the desktop private-beta
pack. Run the bootstrap explicitly before using it.

```powershell
pnpm music:pipeline preflight `
  --plan content/plans/deep-work-calibration-v1.json

pnpm music:pipeline all `
  --plan content/plans/deep-work-calibration-v1.json `
  --run-id deep-work-internal-v1 `
  --flac-output .local/pipeline/deep-work-flac-v1 `
  --opus-output .local/pipeline/deep-work-opus-v1 `
  --pack-id internal.deep-work.v1 `
  --pack-title "Internal Deep Work V1" `
  --flac-version 1.0.0-flac.1 `
  --opus-version 1.0.0-opus.1 `
  --app-version-requirement ">=0.22.0, <0.23.0"
```

If generation has already completed, use the same arguments with `package`
instead of `all`. The FLAC pack remains the lossless internal master; the Opus
pack and adjacent conversion/pipeline reports are distribution candidates. A
conversion failure preserves the verified FLAC pack for diagnosis and retry.
