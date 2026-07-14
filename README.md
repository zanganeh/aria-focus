<p align="center">
  <img src="apps/desktop/src/assets/aria-focus-mark.svg" width="112" alt="Aria Focus ripple mark">
</p>

<h1 align="center">Aria Focus</h1>

<p align="center">
  A private, offline focus-music player for Windows.<br>
  No account. No subscription. No telemetry.
</p>

<p align="center">
  <a href="https://github.com/zanganeh/aria-focus/actions/workflows/ci.yml"><img src="https://github.com/zanganeh/aria-focus/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="MIT OR Apache-2.0"></a>
  <img src="https://img.shields.io/badge/platform-Windows-0078D4" alt="Windows">
  <a href="https://github.com/zanganeh/aria-focus/releases/tag/v0.2.1-beta.1"><img src="https://img.shields.io/badge/status-unsigned%20beta%20preview-orange" alt="Unsigned beta preview"></a>
</p>

Aria Focus is a standalone desktop app for deep work, motivation, creativity,
learning, and light work. It plays integrity-checked music from local storage,
keeps preferences and session history on the device, and presents a deliberately
small activity-first interface.

The project is open for source review and contribution. An unsigned beta preview
is available now with listening-test music. The first reviewed and signed public
build is still being prepared.

## What it includes

- One-click activity tiles for five kinds of focus session
- Play, pause, previous, next, volume, favourites, and keyboard media controls
- Infinite, countdown, and work/break interval timers
- Per-activity intensity, genre, and mood preferences
- Local session history and independent focus/enjoyment feedback
- Fully offline playback after content installation
- Optional **My Music** studio for locally generated instrumental tracks
- Strict manifest, hash, codec, path, and installed-tree validation
- Safe startup recovery without silently deleting user data

## Why it is different

Aria Focus is not a streaming service. There is no account system, cloud library,
advertising, behavioural analytics, or recurring payment. Music and settings stay
on the computer. Bundled content has explicit provenance, technical analysis, and
human-review gates before it can become a public release.

Aria Focus is not medical treatment and does not claim to diagnose or treat ADHD.
It is an independent project and is not affiliated with Brain.fm.

## Download the beta preview

[**Download Aria Focus 0.2.1 Beta 1 for Windows x64**](https://github.com/zanganeh/aria-focus/releases/download/v0.2.1-beta.1/Aria%20Focus_0.2.1_x64-setup.exe)

This 251 MB prerelease is **not code-signed**, so Windows SmartScreen may warn
before installation. Its 100 bundled Opus tracks are listening-test content and
have not completed final public human review. Check the accompanying
[`SHA256SUMS`](https://github.com/zanganeh/aria-focus/releases/download/v0.2.1-beta.1/SHA256SUMS)
before running it. Use the signed release channel when it becomes available if
you do not want to install an unsigned preview.

## Project status

Windows x64 is the supported packaged target. The application, playback engine,
Opus support, content-pack integrity model, timers, history, and local-generation
workflow are implemented and tested. Public distribution remains gated on:

- final human review and redistribution approval for all bundled tracks;
- publication of the immutable reviewed music archive;
- configured Windows code signing; and
- a clean-install and real upgrade test of the signed candidate.

Follow [Releases](https://github.com/zanganeh/aria-focus/releases) for later signed
builds. Do not download installers offered through unofficial mirrors.

## Build from source

### Requirements

- Windows 11 x64
- Node.js 24.11 or newer
- pnpm 10.10
- Rust 1.92 with the MSVC toolchain
- Visual Studio 2022 Build Tools with Desktop C++ support
- Microsoft Edge WebView2 Runtime

The exact verified development environment is recorded in
[`docs/windows-preflight.md`](docs/windows-preflight.md).

### Development app

```powershell
git clone https://github.com/zanganeh/aria-focus.git
cd aria-focus
pnpm install --frozen-lockfile
pnpm tauri dev
```

A normal clone intentionally contains no production music pack, model weights,
or large Music Studio runtime. Source builds use a procedural development sound
until separately reviewed content is staged.

### Quality checks

```powershell
pnpm verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python scripts/check_repository_hygiene.py
```

### Source-only Windows installer

```powershell
pnpm tauri build
```

The resulting NSIS and MSI packages appear under `target/release/bundle/`. They
do not contain the official reviewed music library and are not official releases.

## Music and local generation

Official music is distributed separately from Git because audio binaries are
large and require their own provenance and review lifecycle. Release builds pin
the exact archive name and SHA-256, validate a closed-world manifest, and bundle
only approved assets.

The optional My Music studio lets a user describe the music they want in simple
terms. Generation runs locally after installing a separate runtime and model.
Generated tracks remain local and are clearly separated from reviewed bundled
content. See [`tools/music-generation/README.md`](tools/music-generation/README.md)
for the maintainer-side production and conversion tools.

## Repository map

| Path                         | Purpose                                                       |
| ---------------------------- | ------------------------------------------------------------- |
| `apps/desktop`               | React interface and Tauri desktop host                        |
| `crates/audio-engine`        | Native playback, decoding, looping, DSP, and volume           |
| `crates/catalogue`           | Strict content manifests, imports, and track selection        |
| `crates/domain`              | Session state machine and timers                              |
| `crates/persistence`         | SQLite preferences, history, registry, and migrations         |
| `crates/music-studio-domain` | Local-generation job and validation model                     |
| `tools`                      | Content analysis, ingest, candidate ledger, and music tooling |
| `docs`                       | Architecture, product, safety, content, and release evidence  |

Start with [`docs/architecture.md`](docs/architecture.md) for system boundaries and
[`docs/product-spec.md`](docs/product-spec.md) for product behaviour.

## Releases

GitHub Actions performs ordinary CI on every pull request. Public installers use
a separate, manually approved workflow that:

1. checks out an existing version tag;
2. downloads the exact pinned reviewed-library archive;
3. verifies repository hygiene, content, frontend, and Rust tests;
4. builds NSIS and MSI installers;
5. submits them to SignPath for Windows signing;
6. verifies Authenticode signatures and creates `SHA256SUMS`; and
7. uploads the signed files to a draft GitHub prerelease.

The release remains a draft until a maintainer completes the Windows install and
upgrade matrix. See [`docs/releases.md`](docs/releases.md) and
[`docs/content-pack-upgrades.md`](docs/content-pack-upgrades.md).

Releases are intentionally not automatic. Every push runs CI automatically, but
creating a public release requires a version tag, protected-environment approval,
and an explicit maintainer publication decision. The current unsigned preview was
published manually as a clearly labelled exception to the signed release channel.

## Contributing

Contributions are welcome. Please read [`CONTRIBUTING.md`](CONTRIBUTING.md) before
opening a pull request. Keep changes focused, add tests for behaviour changes, and
never commit generated music, models, runtimes, installers, credentials, or local
agent output.

For vulnerabilities, follow [`SECURITY.md`](SECURITY.md) and use a private GitHub
security advisory instead of a public issue.

## Licence and trademarks

Source code is available under your choice of
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE). Contributions are accepted
under the same terms.

The Aria Focus name, ripple mark, wordmark, and branded installer presentation are
not licensed for use by modified distributions. Forks may use the source under its
open-source licence but must adopt their own name, package ID, icon, and branding.
See [`TRADEMARKS.md`](TRADEMARKS.md), [`ASSETS.md`](ASSETS.md), and
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

Created by **Aria Zanganeh** and Aria Focus contributors.
