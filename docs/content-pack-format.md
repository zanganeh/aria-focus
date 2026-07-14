# Offline content pack format (`.adhdpack` v1)

Phase 1B2a defines a local, ZIP-based interchange format for validated catalogue
metadata and declared audio assets. Validation does not make a pack playable and
does not establish that its publisher is trustworthy.

## Archive layout

The archive has exactly one root manifest and one or more declared assets:

```text
manifest.json
assets/<stable path>.<wav|flac|mp3>
```

`manifest.json` is UTF-8, compact canonical JSON produced by
`catalogue::canonical_manifest_bytes`. Entries outside `manifest.json` and
`assets/`, undeclared files, duplicate or case-ambiguous paths, absolute paths,
traversal, symbolic links, explicit directory entries, and non-file entries are
rejected.

Every archive and manifest path uses one platform-neutral grammar: ASCII only,
at most 240 bytes total and 64 bytes per segment, `/` separators, and segments
containing only letters, digits, `.`, `_`, or `-`. Empty, dot, dot-dot,
leading/trailing-dot, control, colon, backslash, space, and non-ASCII spellings
are invalid. Every segment rejects Windows device basenames case-insensitively,
including when followed by an extension: `CON`, `PRN`, `AUX`, `NUL`, `COM1` to
`COM9`, and `LPT1` to `LPT9`. Canonical case-folded spellings drive duplicate
checks in manifests, archives, and installed trees.

Format identity is `adhdpack`, `format_version` is `1`, pack versions are SemVer,
and `app_version_requirement` is a SemVer requirement that must match the running
application at import time. Stable IDs are lowercase and device independent.

## Required metadata

The manifest owns its genre and mood taxonomies; the UI has no genre or mood
enums. Every content item contains:

- title and stable ID; genre and mood taxonomy references;
- a bounded suitability value for each of Deep Work, Motivation, Creativity,
  Learning, and Light Work;
- source, licence identifier, and composer or generator/model/prompt provenance;
- explicit lyrics and speech declarations;
- technical analysis including duration, loudness/peak/range, spectral and onset
  values, tempo/confidence/drift, section novelty, silence, clipping,
  discontinuity, codec/corruption flags, and vocal/speech likelihood;
- one or more variants with asset path, SHA-256, byte count, codec/sample format,
  safe loop/crossfade regions, and available Off/Low/Medium/High stimulation;
- approved human QA with at least two distinct reviews and representative-work
  review evidence.

Published validation fails closed for missing or invalid fields, non-finite or
out-of-range numbers, unknown taxonomy references, unsafe or duplicate paths,
bad regions, lyrics/speech, vocal/speech likelihood above the named calibration
policy, unexplained silence, clipping, discontinuity, codec error, or corruption.
The constants in `catalogue::manifest::calibration_policy` are conservative
content-ingest gates, not clinical or scientific efficacy thresholds.

## Resource and integrity controls

Default import ceilings are 2 GiB compressed, 4 GiB expanded, 1 GiB per entry,
4 MiB for the manifest, 4,096 entries, and a 200:1 per-entry compression ratio.
They are named implementation limits and may be lowered by a caller. Metadata is
checked before extraction; the manifest itself is read through a hard maximum
plus one byte and checked against its declared ZIP size. Extracted assets are
streamed, counted, and hashed.

An import is staged in the app-data content directory and renamed atomically to
`content/packs/<pack-id>/<SHA-256-of-version>`. Existing targets, installed pack IDs,
overwrites/downgrades, and globally duplicate item IDs are rejected. Files are
installed before a single SQLite registration transaction.

Before the atomic rename, the importer writes and syncs a bounded durable receipt
containing the exact intended registry transaction. After a normal SQLite
commit, the receipt is removed. Startup reconciles crash states: a fully verified
target with a receipt but no registry row is finalized; a matching committed row
with a stale receipt clears the receipt. Missing or corrupt targets and recovery
database failures retain the receipt and return an actionable integrity error.
Untracked targets without receipts are surfaced and never silently deleted. An
ordinary registration error removes only the just-installed computed target and
its receipt; failure of rollback is reported. Startup also revalidates the
install root and assets root against symlinks and Windows reparse points. The
install root is closed-world: its direct entries must be exactly one plain
`manifest.json` file and one plain `assets` directory, with no extra files,
directories, links, receipts, or case aliases. It then checks every installed
relative path, canonical manifest, registry field, asset size, and hash.

SQLite paths are never trusted as filesystem capabilities. Startup first bounds,
parses, validates, canonicalizes, and hashes the manifest stored in the registry;
derives the only permitted target below `content/packs`; and compares the stored
path, identity, title, version, count, status, and hashes. Any mismatch is
rejected before filesystem metadata or file reads. Installed verification then
uses only the derived target, never the stored path.

On Unix, receipt creation/removal syncs the receipt-directory entry and the
post-rename install operation syncs the target's parent directory. Windows does
not use ordinary file handles for directory `fsync`; V1 explicitly relies on the
synced receipt file plus startup reconciliation there. This is crash recovery,
not a guarantee against storage hardware or filesystem failure.

## Local ingest tool

```powershell
cargo run -p content-ingest -- `
  --source <source-directory> `
  --manifest <author-manifest.json> `
  --output <name.adhdpack>
```

The source directory must contain each declared `assets/...` path. The tool
rejects missing, symlinked, or out-of-root files, computes byte counts and
SHA-256 values, validates and canonicalizes metadata, and creates a new pack
without overwriting an existing output. It does not download, compose, generate,
decode, or analyze music; authors must supply truthful analysis, licence,
provenance, and completed human-QA metadata.

## Playback contract and trust limitations

At every session start, the pack service repeats registry preflight, manifest
validation, app compatibility, closed-world tree inspection, size checks, and
SHA-256 verification before selection. Eligible assets are WAV PCM, FLAC, or
MP3, mono or stereo, with an authored continuous region and all four stimulation
levels. Although the schema retains the `ogg_opus` enum value for explicit
diagnostics, published validation rejects it in format version 1 because the
pure-Rust playback stack does not include an Opus decoder.

Selection is deterministic and activity-specific. It uses approved QA,
positive suitability, playable technical fields, safe regions, and stable IDs;
the immediately previous item is avoided whenever at least two distinct items
are eligible. Each encoded file is bounded to 512 MiB and the aggregate decoded
program to 64 Mi samples. The exact opened handle is hashed, rewound, decoded,
and checked against the declared technical fields. Failure is visible and does
not start or partially commit a session. With no eligible installed content,
the app labels and uses its procedural fallback explicitly.

- SHA-256 proves that imported bytes match the supplied manifest; it does not
  authenticate the author. V1 has no signatures, certificate chain, publisher
  allow-list, revocation, or licence-document archival. Treat unknown packs as
  untrusted even when structurally valid.
- Automated analysis fields and human-review records are validated for shape and
  policy compliance but are self-asserted. Independent analysis and reviewer
  identity verification remain future work.
- Validation and decoding make an asset technically playable; they do not prove
  the publisher's claims about quality, ownership, or functional effectiveness.
- No starter music catalogue, Brain.fm audio, private API, brand asset, or
  proprietary generation method is included.
