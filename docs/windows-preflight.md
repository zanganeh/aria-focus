# Windows development preflight

Verified on 10 July 2026 in `D:\projects\personal\adhd-music`.

## Available

| Component | Verified state |
| --- | --- |
| Git | 2.47.1.windows.1 |
| Node.js | 24.11.1 |
| npm | 11.4.2 |
| pnpm | 10.10.0 |
| rustc / Cargo | 1.92.0, stable MSVC toolchain |
| Rust host | `x86_64-pc-windows-msvc` |
| Visual Studio Build Tools | 2022 17.14.24 with x64/x86 C++ tools |
| Edge WebView2 Runtime | 150.0.4078.48 |
| CMake | 3.31.6 |
| FFmpeg / ffprobe | 7.1 full build |
| GPU | NVIDIA GeForce RTX 3060, 12 GB VRAM |
| Free workspace-drive capacity | Approximately 1.55 TB on D: |

This proves that the host has the native compiler, Rust target, WebView runtime,
media tooling, and storage needed for a Windows Tauri/audio build. The GPU may be
useful for testing local music-generation tools, but local generation is not in
the real-time application path or Phase 1 scope.

## Remaining platform prerequisites

- No global `cargo-tauri` or `tauri` command is installed.
- No Java/Android SDK, Android target, or Apple toolchain was verified.
- No Windows code-signing certificate was verified.

The repository has since been initialised with Git, and the pinned workspace
tooling has produced the Phase 0 Windows executable, MSI, and NSIS installer.

A global Tauri CLI is unnecessary. The implementation should pin the JavaScript
CLI in the workspace and invoke it through pnpm, keeping the developer and CI
versions reproducible.

## Implementation-agent bootstrap

The development agent should:

1. Initialise Git without deleting `.codebase-memory` or the specification files.
2. Create the pnpm and Cargo workspaces described in `docs/architecture.md`.
3. Pin tool versions in `package.json`, `pnpm-lock.yaml`, and `rust-toolchain.toml`.
4. Add the Tauri 2 CLI as a workspace development dependency.
5. Build the smallest Windows shell before adding application features.
6. Add formatting, linting, tests, and Windows packaging to CI immediately.
7. Record exact generated tool versions and any departure from the architecture.

The implementation must not install unpinned global CLIs or overwrite the
research/specification documents during scaffolding.
