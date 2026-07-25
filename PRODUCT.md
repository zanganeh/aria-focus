# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Primary users are people with ADHD or other focus difficulties who are working,
studying, or doing another sustained-attention activity and want a clear,
distraction-free focus session. They need to choose an activity, start music and
a timer quickly, and keep the session running with minimal cognitive load.

## Product Purpose

Aria Focus provides focused work sessions built around instrumental music,
activity-based soundscapes, timers, playback controls, and local session
history. Success means a user can start and maintain a useful focus session
without an account, subscription, telemetry, or a network connection for normal
listening.

## Positioning

Aria Focus is a free, open-source desktop focus-music app that combines an
offline listening experience with an optional AI Music Studio. Users can listen
privately to locally installed music and, when they choose, create additional
instrumental tracks using either the supported local generation path or their
own OpenRouter account. The user retains control of the API key, generation
budget, candidates, previews, and activation of new music.

## Operating Context

The core workflow is activity-first: choose a focus activity such as Deep Work,
Motivation, Creativity, Learning, or Light Work; choose available sound and
timer options; then play, pause, skip, adjust volume, and complete the session.
Users may review favourites, inspect local session history, and open Create for
AI music generation. Generated batches are previewed and reviewed before they
are saved to the personal library or activated for listening.

## Capabilities and Constraints

- Desktop application built with Tauri, React, and a Rust backend; Windows and
  macOS packages are supported.
- Normal listening is offline and uses integrity-checked local content. A
  source checkout intentionally does not include the production music pack;
  content installation and validation are separate release concerns.
- The app provides activity selection, instrumental playback, timers,
  previous/next controls, volume, favourites, keyboard media controls, feedback,
  and session history.
- AI Music Studio supports local generation on supported Windows/NVIDIA hardware
  and optional OpenRouter cloud generation. Local generation requires the
  reviewed runtime and hardware prerequisites; it is not assumed to work on
  every platform.
- OpenRouter generation is opt-in. The user supplies the key, selects a model,
  track count, duration, and maximum budget; the app should use current model
  pricing for estimates and report provider errors without replacing the
  previously active library.
- OpenRouter credentials belong in the operating-system credential store and
  must not be placed in preferences, prompts, logs, or release artifacts.
- Generation is a background operation with persistent status, preview, save,
  discard, cancellation, and explicit activation/review gates. Test-only mock
  generation exists for local flow testing and is disabled by default.
- Generated focus music is instrumental: no lyrics, vocals, or speech. The
  established prompt direction favors warm, deep, high-end, purposeful music;
  avoid jazz, harsh/high-pitched leads, noisy or toy-like results, and long
  introductions. These are content-quality constraints, not visual rules.
- Aria Focus is not medical treatment and does not claim to diagnose or treat
  ADHD.
- Product messaging must remain independent and must not imply affiliation with
  a named third-party focus-music service.
- Open decisions: the long-term hosted generation provider/model strategy and
  the exact production music catalogue are still subject to R&D and review.

## Brand Commitments

- The product name is Aria Focus.
- It is presented as free, open-source, private, offline-first focus software
  with an optional AI Music Studio.
- The existing Aria Focus ripple mark and application identity are source assets
  in `apps/desktop/src/assets/` and the Tauri bundle configuration.
- Product copy should be direct, calm, clear, and honest about platform,
  generation, pricing, content availability, and unsigned release status.

## Evidence on Hand

- Product README: `README.md`.
- Main desktop shell: `apps/desktop/src/App.tsx`.
- AI Music Studio UI: `apps/desktop/src/components/CloudGenerationPanel.tsx`.
- Prompt system and R&D notes: `docs/music-prompt-system.md` and
  `docs/music-generation-rd.md`.
- Local generation maintainer documentation:
  `tools/music-generation/README.md`.
- Existing logo asset: `apps/desktop/src/assets/aria-focus-mark.svg`.
- The repository contains automated frontend and Rust tests, but no fabricated
  testimonials, customer studies, or external performance claims should be
  added without evidence.

## Product Principles

1. Start a useful focus session with minimal friction.
2. Keep normal listening private, local, and dependable.
3. Make generation costs, prerequisites, progress, and failures explicit.
4. Let users review and control generated content before it affects listening.
5. Treat accessibility and cognitive simplicity as product requirements.

## Accessibility & Inclusion

The experience should be ADHD-friendly: low reading load, clear primary actions,
predictable navigation, persistent background-job status, and actionable error
messages. Controls must remain usable with keyboard and assistive technology,
and important states such as playing, paused, generating, unavailable, and
failed must not rely on color alone.
