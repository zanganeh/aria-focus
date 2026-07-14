# ADR 0009: Native portability CI foundation

## Status

Accepted — 2026-07-11

## Context

The Rust workspace contains CPAL, Tauri, and platform-specific filesystem identity code. Before
this decision, native Rust formatting, linting, tests, and Tauri packaging ran only on Windows.
The Ubuntu web job did not compile native Rust, CPAL, or Tauri code, so it was not evidence of
native portability.

## Decision

Keep Windows as the only packaging and release-artifact target. Add a `native-portability` GitHub
Actions matrix for `ubuntu-latest` and `macos-latest`, using Rust 1.92.0. Each matrix host runs:

- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

Ubuntu installs the documented Tauri v2 native prerequisites for checks/tests plus CPAL's ALSA
development package: `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `libwebkit2gtk-4.1-dev`, and `libasound2-dev`.
Installation is noninteractive and deliberately does not use obsolete WebKit 4.0 packages.

The dependency list follows the [Tauri v2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/)
and [CPAL Linux build dependencies](https://github.com/RustAudio/cpal#linux-build-dependencies),
checked 2026-07-11.

Tests remain headless through the existing fake audio-output and application-service test seams;
the matrix never starts the Tauri GUI.

## Consequences

The workflow is configured to prove that the complete Rust workspace natively compiles, passes
warning-denied Clippy, and passes its headless tests on Windows, Linux, and macOS. Matrix execution
becomes evidence only after the workflow is pushed and runs successfully.

It does not prove runtime audio-device behaviour, Tauri UI smoke behaviour, signing/notarization,
or Linux/macOS bundles or installation. No Linux or macOS release-readiness claim follows from
this decision.
