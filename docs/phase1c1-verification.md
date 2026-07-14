# Phase 1C1 installed-playback verification report

Date: 10 July 2026

Status: validated installed WAV, FLAC, and MP3 assets can be decoded,
resampled, selected, transitioned, paused, and played by the native engine.
This is an engineering milestone, not the reviewed music catalogue or finished
Focus MVP.

## Independently verified outcome

- The pack service revalidates the registry and installed tree before every
  selection, then hashes and decodes the same open file handle.
- Published format version 1 accepts exact supported WAV PCM/float, FLAC, and
  MP3 codecs. It checks byte count, canonical SHA-256, sample rate, channels,
  bit depth, finite decoded samples, duration, and bounded aggregate memory.
- Eligible content is selected deterministically for the active Focus activity.
  Every playable variant must support Off, Low, Medium, and High stimulation.
- Authored loop regions repeat without a gap. Two-track programs crossfade
  between authored outgoing and incoming regions, preserve stereo channel
  order, alternate continuously, and continue after the incoming region.
- Program-wide headroom is calculated before the callback from the dry tracks
  and their actual overlaps. Correlated and opposite-polarity full-scale test
  material remains bounded without a transition-only hard-clamp dip.
- The callback publishes the actual current track without locks or allocation.
  Pause fades to silence and then freezes the source cursor; resume continues
  from the same point. The UI polls the current source while playing or paused.
- Stop and timer expiry commit the item that was actually current. If no
  installed item satisfies the activity and continuous-playback contract, the
  procedural source is explicitly labelled as a fallback.

## Review and correction record

GLM-5.2 performed development work. GPT-5.6 Sol independently reviewed and
reproduced the verification gates. Review identified and corrected the
following blocking issues before this report:

- one-sided crossfades that ignored the incoming authored safe region;
- transition hard-clamping that could hide overlap clipping;
- overly broad WAV codec acceptance and incomplete media metadata checks;
- per-track bounds without an aggregate decoded-program limit;
- a source label that remained on the primary item after track alternation;
- pause muting that allowed the source timeline to continue advancing;
- a hash-verification/reopen substitution window;
- missing real WAV PCM16/PCM24/float, FLAC, MP3, mono/stereo, and
  44.1/48/96 kHz decode fixtures; and
- UI wording that overstated what pack validation guarantees.

The final valid-pack service test was also strengthened during independent
review to prove that its authored loop region survives import and decode.
Strict Clippy then found two test-helper needless borrows; those were corrected
before the full suite was accepted.

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

- Rust: 106 tests passed: 31 desktop/coordinator/pack-service, 32 audio-engine
  unit, 8 real-media decode-matrix, 16 catalogue/import, 2 ingest, 10 domain,
  and 7 persistence tests. No failures or ignored acceptance tests.
- Frontend: 37 tests passed across 11 files, including live current-source
  polling. Prettier, ESLint, TypeScript, and the production Vite build passed.
- Workspace check and Clippy with every warning denied passed.
- A service-level test constructs a valid `.adhdpack` around the committed
  1-second PCM16/44.1 kHz mono fixture, imports it, selects it for Deep Work,
  decodes exactly 44,100 samples, and preserves `Loop(0.1s..0.9s)`.
- The committed decode corpus exercises PCM16 WAV, PCM24 WAV, float WAV, FLAC,
  MP3, silence, mono/stereo, and 44.1/48/96 kHz resampling paths.
- Tauri produced the standalone executable and both Windows installer formats.

## Windows runtime evidence

The exact release executable below was launched on Windows after packaging. It
remained alive and responsive after five seconds and created a native window
with a non-zero handle and the title `ADHD Music — Focus`; the verification
process then closed that exact PID.

The Windows UI-control connection was unavailable for a fresh visual
click-through in this resumed verification session. The preceding Phase 1B2b
report contains the independently observed Countdown/start/stop visual smoke.
Phase 1C1 changed production playback and source reporting, while the final
post-build change in this session was test-only. A listener-grade installed
music pack has not yet been created, so no claim of audible music quality is
made here.

## Current Windows artifacts

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/release/adhd-music-desktop.exe` | 8,392,192 | `26537BD2FD52BB0C6F8AC469F8C72FA5AE8703BAAFF7A8AAE9A58B75F831D31C` |
| `target/release/bundle/msi/ADHD Music_0.1.0_x64_en-US.msi` | 3,952,640 | `E48D7F149333AAA3EF06E6C51771AC51970948BDE4E6E1E978AD3A2C27E5C52B` |
| `target/release/bundle/nsis/ADHD Music_0.1.0_x64-setup.exe` | 2,832,461 | `E614ABF6F6EAF430AB934D96DC49E6A460ADF28ED83822D0023D4EB9DAF40A6A` |

The packages are not code-signed and are not asserted to be byte-for-byte
reproducible.

## Remaining limitations

- The repository contains technical decoder fixtures, not a legally reviewed,
  listener-tested starter music library. The procedural tone remains the only
  bundled source.
- No representative ADHD/focus work-session listening panel has accepted the
  musical dynamics, distraction level, fatigue, transitions, or stimulation
  profiles. The software must not claim ADHD treatment or medical benefit.
- Installed programs are predecoded into memory. Hard aggregate bounds exist,
  but streaming decode and a lower steady-state memory footprint remain future
  work.
- Current-source display is polled every 500 ms, and immediate-repeat history is
  process-local rather than durable personalization.
- Ratings, favorites, preference learning, automated content analysis, device
  loss/default-device recovery, suspend/resume recovery, a final safety
  limiter, dependency/licence audit, signing, installer lifecycle tests, and
  non-Windows packaging remain incomplete.
- Patent and freedom-to-operate review is required before public or commercial
  distribution of neural-stimulation DSP.

