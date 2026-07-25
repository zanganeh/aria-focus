# Local ACE-Step production bootstrap

## Metadata-driven prompt generation

The provider-neutral taxonomy and deterministic prompt construction rules are
documented in [docs/music-prompt-system.md](../../docs/music-prompt-system.md).
To rebuild the sanitized catalog and one example prompt from a private session
HAR, run:

```powershell
python -B tools/music-generation/derive_prompt_catalog.py `
  --har "C:\path\to\session.har" `
  --output content/music/prompt-catalog-v1.json `
  --example-output content/music/examples/motivation-electronic-profile-001.json `
  --profile-id profile-001 `
  --activity Motivation
```

The output contains only musical metadata and prompt profiles. Never commit a
raw HAR or any file containing authentication, cookies, signed URLs, or audio
URLs.

For the prompt R&D pilot, first translate the model-neutral review matrix into
the strict production plan:

```powershell
python tools/music-generation/build_prompt_pilot_plan.py --protocol experiments/music-generation/motivation-prompt-pilot.json --base-plan content/plans/motivation-multigenre-calibration-v1.json --output content/plans/motivation-prompt-pilot-v1.json
```

The translator refuses to overwrite an existing plan. The resulting plan is
then run through the same preflight, generation, analyzer, provenance, and
candidate-ledger path as every other production batch.

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

## OpenRouter cloud-generation workflow

The desktop app also supports a paid, user-owned OpenRouter workflow for the
quality pilot and replacement-library candidates. It is separate from the
offline ACE-Step bootstrap above and never runs automatically during normal
listening.

1. Open **Settings → OpenRouter**, paste a temporary or personal API key, and
   save it. The key is stored in the operating-system credential store; it is
   not written into the repository or app preferences.
2. Open **Create**, choose activities, track count, duration, and the live
   model prices, then review the maximum budget before confirming. The default
   audio model is `google/lyria-3-pro-preview`; the app displays the live
   model catalogue and the published media rate when available.
3. Confirm the batch. Generation, prompt refinement, cover art, and local
   technical checks run in the background. Leaving Create or opening Settings
   does not cancel the job. Test-only mock generation is available only in a
   mock-feature build and is rejected by the export tool.
4. Preview every candidate in the app and inspect the saved structured prompt.
   Analyzer flags are deliberately retained for review. They do not make a
   candidate release content, and activation remains fail-closed while any
   hard technical rejection is present.

For a repeatable local run without the desktop UI, the same service is
available as a guarded CLI. It uses the app's existing credential-store key and
the same SQLite/content paths. It prints the estimate before creating a batch;
`--confirm-paid` is required for a non-test provider, and `--wait` keeps the
process attached until the background batch reaches a terminal state:

```powershell
cargo run -p aria-focus-desktop --bin cloud-generation-cli --no-default-features -- `
  --activities motivation `
  --count 1 `
  --duration 180 `
  --budget-usd 0.10 `
  --wait `
  --confirm-paid
```

Use `--activate` only after listening to the returned candidate. The CLI never
prints or accepts the API key itself.

For a stopped batch, pass the original `--count` and a new ceiling with
`--resume-batch BATCH_ID`. Completed items are preserved and only interrupted
items are retried. Transient audio transport failures are retried up to three
times with a short backoff; authentication, model, policy, and insufficient
credit errors stop immediately with a safe explanation. The estimator uses a
conservative `$0.04` per generated cover when OpenRouter's model catalogue
exposes only token pricing for the image model; the final charge is still the
provider-reported usage.

5. Export a validated cloud batch into a new FLAC draft pack. This is a
   staging operation only; it does not approve, activate, or publish content:

```powershell
python tools/music-generation/export_openrouter_batch.py `
  --db "$env:APPDATA\com.ariazanganeh.ariafocus\preferences.sqlite3" `
  --storage-root "$env:APPDATA\com.ariazanganeh.ariafocus\content" `
  --batch-id cloud_batch_XXXXXXXXXXXX `
  --output .local/openrouter/motivation-draft `
  --licence-id openrouter-provider-terms-2026-07 `
  --licence-url https://openrouter.ai/docs/terms
```

The command keeps each structured prompt, model ID, provider batch ID,
audio/cover hashes, and analyzer report in the manifest. It rejects mock
output, missing/raster-invalid cover art, path escapes, hash mismatches,
failed cover attempts, and non-validated items.

The exporter keeps the provider file for provenance and applies a deterministic
`-3 dB` gain step to each FLAC release master. This gives the distribution
master safe headroom when a provider response contains a small number of
full-scale samples; unrelated analyzer hard rejections remain fail-closed.

For a replacement run, a rejected item may be omitted temporarily with one or
more `--exclude-item-id` arguments. This option is staging-only: it must never
be used to create the final pack. Generate the replacement candidate, then use
`replace_openrouter_item.py` to copy the base pack and put the replacement back
under the original stable item ID. The replacement command refuses activity
mismatches, missing hashes, invalid media, and in-place edits. Always rerun the
full exporter audit after all replacements are merged.

Example replacement merge:

```powershell
python tools/music-generation/replace_openrouter_item.py `
  --base-pack .local/openrouter/base-flac `
  --replacement-pack .local/openrouter/replacement-flac `
  --base-item-id cloud-item-cloud-batch-XXXXXXXXXXXX-5 `
  --replacement-item-id cloud-item-cloud-batch-YYYYYYYYYYYY-0 `
  --output .local/openrouter/merged-flac `
  --pack-version 2.0.0-draft.1
```

6. Have two distinct human reviewers listen to each candidate, including a
   representative session in the actual app. Repair or reject clipping,
   unexplained silence, speech/vocals, harsh high frequencies, repetition,
   weak musical identity, or unsuitable activity fit. Record the reviewer
   evidence in the manifest; automated scores cannot replace either reviewer.
7. Only after all 100 items have two approvals, no unresolved analyzer
   rejection, valid licence evidence, and exactly 20 tracks per activity,
   convert the FLAC master pack to the fixed Opus distribution pack, run the
   public verifier, build the reproducible archive, update the immutable
   release pin, and run the unsigned stable-release workflow. Never copy the
   draft directly into the shipped pack.

The current local candidate is a 100-track OpenRouter replacement draft at
`.local/openrouter/replacement-v2-flac-final3`, with its 100-track Opus
distribution candidate at `.local/openrouter/replacement-v2-opus-v2`. It is
technically complete but remains unpublished until two distinct human
reviewers approve every item and the public verifier passes. Do not promote a
pack merely because it is technically playable.

## Ogg Opus candidate distribution

`convert_library_to_opus.py` makes a new, closed-world candidate pack from a
closed-world FLAC-master pack. It never modifies the masters or an existing
destination. It emits Ogg Opus at fixed 48 kHz stereo, 112 kbps VBR using
`ffmpeg`'s `libopus` encoder, updates each existing item ID and manifest asset
hash/size/codec/path, validates each emitted file using `ffprobe`, and enforces
a 500 MB default package budget. `ffmpeg` and `ffprobe` are required locally.

First validate the full-library paths without writing anything:

```powershell
python tools/music-generation/convert_library_to_opus.py --source apps/desktop/src-tauri/private-beta-pack --output .local/opus-library-candidate --pack-version 0.3.0-opus.1 --app-version-requirement '>=0.3.0, <0.4.0' --dry-run
```

Then generate the staged candidate and its adjacent audit report. The source
pack is only read; the new directory must not already exist.

```powershell
python tools/music-generation/convert_library_to_opus.py --source apps/desktop/src-tauri/private-beta-pack --output .local/opus-library-candidate --pack-version 0.3.0-opus.1 --app-version-requirement '>=0.3.0, <0.4.0' --max-total-bytes 500000000
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
  --app-version-requirement ">=0.3.0, <0.4.0"
```

If generation has already completed, use the same arguments with `package`
instead of `all`. The FLAC pack remains the lossless internal master; the Opus
pack and adjacent conversion/pipeline reports are distribution candidates. A
conversion failure preserves the verified FLAC pack for diagnosis and retry.
