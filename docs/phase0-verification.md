# Phase 0 verification report

Date: 10 July 2026

Status: Phase 0 Windows vertical slice verified; full Focus MVP not complete.

## Scope verified

- Tauri 2 desktop application launches on Windows 11 Pro x64, build 22631.
- Deep Work session starts, advances its timer, pauses without advancing,
  resumes, and stops without crashing.
- Off, Low, Medium, and High / ADHD intensity choices are exposed to Windows UI
  Automation as named radio controls.
- High / ADHD can be selected with a pointer, and native left-arrow navigation
  moves selection back to Medium with a visible focus state.
- The Rust renderer produces the loop consumed by Web Audio, and changing
  intensity uses the implemented crossfade path.
- MSI and NSIS packages build successfully from the pinned workspace.

## Independently reproduced automated evidence

The following commands completed successfully in the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm verify
pnpm tauri build
```

Results:

- Rust: 19 tests passed (10 audio engine, 9 domain integration); no failures.
- Frontend: 17 tests passed across 3 files; no failures.
- Prettier, ESLint, TypeScript, Cargo formatting, and Clippy: clean.
- Vite production build: 40 modules transformed successfully.
- Tauri release build: application, MSI, and NSIS bundles produced successfully.

## Final build artifacts

The hashes below are the historical Phase 0 Web Audio build. Phase 1A later
replaced the same output paths; see `phase1a-verification.md` for the current
native-audio artifacts.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/release/adhd-music-desktop.exe` | 4,264,448 | `1F8D87D21C5AB765BA907D48225BC277A5983F58F5A76AD99AD2D2A525BE04E6` |
| `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi` | 2,002,944 | `33159975110D759E8A528243A14187A617E0BE0720D6BA527FCB0695201C60D0` |
| `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe` | 1,365,988 | `908FB6506B09FE2F6705FF3586230BC95873E945157EF37739B420A6374DE174` |

These hashes describe this local build only. The bundles are not code-signed,
and the current packaging process is not asserted to be reproducible byte for
byte.

## Honest limitations

- This is a procedural test tone, not a high-quality music catalogue.
- Playback uses Web Audio with Rust-rendered WAV buffers. The planned native
  CPAL real-time backend, device switching, output limiter, and suspend/resume
  recovery remain Phase 1 work.
- The runtime test proved control state, timing, rendering, and the Windows
  accessibility bridge. It did not constitute a human listening-quality review
  or prove audible output on every enumerated audio device.
- Long-session, sleep/wake, Bluetooth, HDMI, upgrade/uninstall, code-signing,
  dependency-audit, and multi-platform verification remain outstanding.
- No clinical efficacy is claimed. Patent and legal review is still required
  before public or commercial distribution of stimulation DSP.
