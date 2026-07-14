# Phase 1B2a catalogue and content-pack verification report

Date: 10 July 2026

Status: Secure metadata catalogue/content-pack boundary verified; decoded music
playback and the reviewed starter catalogue are not complete.

## Independently verified outcome

- `.adhdpack` v1 is a versioned, canonical ZIP format with fail-closed metadata
  validation for taxonomies, five-activity suitability, provenance/licence,
  technical analysis, safe regions, stimulation availability, and human QA.
- Import accepts only canonical, declared, hash-matching assets under a strict
  cross-platform ASCII path grammar. It rejects traversal, links/reparse
  points, Windows device names, case aliases, directory entries, undeclared
  payloads, compression/size abuse, corruption, speech/lyrics, clipping, and
  missing QA/provenance.
- Files stage under app data and install through atomic rename. A synced durable
  receipt and startup reconciliation cover crashes between rename and SQLite
  registration. Corrupt, incomplete, or untracked targets fail visibly and are
  not silently deleted.
- Registry rows are fully checked against their bounded canonical manifest and
  derived app-data target before any installed filesystem path is accessed.
- Installed roots are closed-world: exactly one plain `manifest.json` and one
  plain `assets` directory, followed by recursive size/hash/path checks.
- The local authoring CLI hashes and packages supplied assets without network,
  generation, download, overwrite, or audio modification.
- The Windows UI uses the scoped official file dialog, lists only pack title,
  version, and item count, and states `Validated metadata; playback not
  implemented yet.`

## Review defects found and corrected

Independent review required two hardening rounds. The initial implementation
allowed Windows path aliases/non-ASCII collisions, trusted the ZIP-declared
manifest size, did not reject a linked assets root, and lacked crash recovery
after rename. Those were corrected with canonical paths, bounded reads, root
reparse checks, durable receipts, recovery tests, and untracked-target audits.

A second review found that a corrupt registry path was dereferenced before
comparison with the expected target and that undeclared pack-root payloads were
not audited. Registry validation now precedes all installed-tree access and the
pack root is exact/closed-world.

## Reproduced automated evidence

The final independent run used `CARGO_BUILD_JOBS=1` after a prior Windows UI
helper leaked memory during agent-side testing.

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm verify
pnpm tauri build
```

Results:

- Rust: 67 tests passed (21 desktop/coordinator/import service, 14 audio, 13
  catalogue security, 12 domain, 5 persistence, 2 ingest CLI); no failures.
- Frontend: 29 tests passed across 8 files; no failures.
- Formatting, workspace check, strict Clippy, Prettier, ESLint, TypeScript, and
  production Vite build: clean.
- Tauri release build: executable, MSI, and NSIS bundles produced successfully.
- Runtime smoke: final EXE launched, empty content-pack state and limitation
  were visible/accessibly named, native chooser opened, and Escape cancelled it
  without an error or import.

## Current Windows artifacts

These hashes are historical evidence for the Phase 1B2a build. The current
artifacts are recorded in `docs/phase1b2b-verification.md`.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/release/adhd-music-desktop.exe` | 6,774,784 | `03D76383F20C487FE7BC4FDB4F6DC06E9F8616BF26E99D08E0A80F36D9B71B2C` |
| `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi` | 3,280,896 | `CC43C1F9F6F186F7ADB616986EAA0296E165A4054B3C36582DDFE765EC09A0FE` |
| `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe` | 2,362,240 | `18D17D7A983D3D9E72FBE712FD4AB2CCBF2FEA03033D882AA44EBE20585C9FEC` |

The packages are not code-signed and are not asserted to be byte-for-byte
reproducible.

## Remaining limitations

- Imported metadata and assets are not decoded or playable yet.
- Pack hashes verify integrity against a self-supplied manifest, not publisher
  identity. V1 has no signatures, trust store, revocation, or archived licence
  documents.
- Analysis and human-review records are structurally validated but still
  self-asserted; independent media analysis and real reviewer identity remain
  Phase 1C work.
- No real music, starter pack, automated audio analyzer, or human focus-quality
  listening report exists yet.
- There is no user-facing pack uninstall/repair workflow yet.
- Ratings/favorites, decoder/resampler/gapless queue, device recovery, and
  release signing remain outstanding. Timer UI was completed in Phase 1B2b.
