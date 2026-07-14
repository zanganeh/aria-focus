# Phase 1B1 activity and persistence verification report

Date: 10 July 2026

Status: Five-activity and local-preference slice verified; catalogue/timer MVP
not complete.

## Independently verified outcome

- Deep Work, Motivation, Creativity, Learning, and Light Work appear as named
  native radio controls with the product-spec descriptions and sound directions.
- Activity controls become disabled during native CPAL playback, and the Rust
  domain independently rejects active-session activity changes.
- The UI states that every activity still uses the same procedural test source;
  it does not imply that activity-specific music already exists.
- A bundled SQLite database in Tauri's app-data directory stores the last
  activity and a separate intensity for each activity through a numbered
  migration and validated storage keys.
- Coordinator tests cover ordering and rollback across domain, audio, and
  persistence boundaries. Failed audio changes are not persisted, and failed
  database writes restore the previous audio/domain preference.
- Native command errors are caught and rendered through a dismissible live
  alert instead of becoming unhandled frontend rejections.

## Runtime persistence evidence

On the Windows release executable, the independent test selected Creativity
and High / ADHD, started native playback, confirmed the Creativity radio was
disabled while Playing, stopped, closed the process, and relaunched it. The
new process restored Creativity and High / ADHD visually, changed the Start
label to `Start Creativity`, and showed no error banner.

## Reproduced automated evidence

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm verify
pnpm tauri build
```

Results:

- Rust: 40 tests passed (14 audio engine, 12 domain, 11 coordinator, 3
  persistence); no failures.
- Frontend: 24 tests passed across 6 files; no failures.
- Cargo check, formatting, strict Clippy, Prettier, ESLint, and TypeScript: clean.
- Vite production build: 42 modules transformed successfully.
- Tauri release build: executable, MSI, and NSIS bundles produced successfully.

## Current Windows artifacts

The hashes below are the historical Phase 1B1 build. Phase 1B2a later replaced
the same output paths; see `phase1b2a-verification.md` for the current secured
catalogue/import build.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/release/adhd-music-desktop.exe` | 5,983,232 | `D506D19A80F3AB70BC0BE0C7C17B53716B1D2290C7B6509B4F3E2FFB758ABE7A` |
| `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi` | 2,928,640 | `E2108ED27871AFE373A62088F637FA97C058D990009230042A03356A2D9418DF` |
| `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe` | 2,106,909 | `1B06DED1F695FC322ABDAE2CEE690C6CB594FB5E00D073D15350C7F01C08237F` |

The packages are not code-signed and are not asserted to be byte-for-byte
reproducible.

## Remaining limitations

- Activity choice changes context and preferences only; it does not yet change
  composition, genre, mood, energy, or instrumentation.
- There is no metadata catalogue, decoded music asset, content-pack import,
  timer-choice UI, favorite, rating, or personalization selector yet.
- The SQLite migration framework does not yet carry migration checksums or
  backup/repair tooling; invalid values fail visibly instead of being rewritten.
- Phase 1A device-switching, unplug, sleep/wake, stream-recovery, limiter, and
  human listening-quality work remain outstanding.
- No medical efficacy is claimed, and patent/legal review is still required
  before public or commercial stimulation-DSP distribution.
