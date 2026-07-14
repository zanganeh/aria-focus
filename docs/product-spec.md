# Product specification

## Outcome

The user can begin a useful focus session in at most three deliberate choices:

1. Choose an activity.
2. Choose or retain a preferred sound profile.
3. Press Play.

Advanced controls remain available without making the default path demanding.
The player must supply stimulation without introducing lyrics, abrupt changes,
or repeated interface decisions.

## Initial audience and positioning

The initial user is an adult who already knows that structured background audio
helps them work, including users with ADHD or attention difficulties. The app is
a productivity and environmental-support tool, not an ADHD treatment, diagnostic
tool, neurofeedback device, or medication replacement.

## Core selection model

The application models each session using independent axes rather than treating
all choices as playlists.

### Mental state

Version 1 supports `Focus`. The domain model reserves `Relax`, `Sleep`, and
`Meditate` so they can be added without changing stored session records.

### Focus activity

| Activity | User need | Default sound direction |
| --- | --- | --- |
| Deep Work | Sustained, cognitively demanding work | Low salience, stable density, medium energy |
| Motivation | Starting avoided or low-reward tasks | Brighter and more rhythmic, controlled energy |
| Creativity | Open-ended writing, design, and ideation | Spacious, gently evolving, less rigid pulse |
| Learning | Reading, comprehension, and retention | Sparse arrangement, minimal melodic competition |
| Light Work | Email, filing, and repetitive administration | Pleasant, slightly more varied, lower stimulation |

The descriptions above are our product definitions. They are not claims about
Brain.fm's undisclosed per-activity audio recipes.

### Sound profile

A sound profile has two user-facing dimensions:

- Genre: Atmospheric, Lo-Fi, Electronic, Piano, Classical, Acoustic, Cinematic,
  Drone, Grooves, Post-Rock, or Nature.
- Mood tags: Calm, Warm, Dark, Dreamlike, Driving, Energising, Hopeful, Meditative,
  Mysterious, Playful, Serene, Upbeat, and Uplifting.

Genres and moods are metadata, not hard-coded screens. The catalogue may add
values without an application release.

### Stimulation intensity

| Level | Label | Behaviour |
| --- | --- | --- |
| 0 | Off | Source audio without stimulation processing |
| 1 | Low | Subtle effect for sensitive listening or light work |
| 2 | Medium | Default functional-audio profile |
| 3 | High / ADHD | Strongest profile; opt-in and never assumed to be universally better |

The setting controls DSP parameters, not master playback volume. The product
must explain that individual response varies and allow instant switching or Off.

### Session type

- Infinite: counts elapsed focus time and continues until stopped.
- Countdown: plays for a chosen duration and ends gently.
- Interval: configurable work and break durations, repeat count, and optional
  break audio. Defaults to 25 minutes work and 5 minutes break.

## Primary user flow

### First run

1. Show a concise non-medical explanation of functional audio.
2. Ask whether the user is sound-sensitive, usually needs ordinary stimulation,
   or often needs strong stimulation. Use this only to select Low, Medium, or
   High as a starting preference.
3. Ask the user to select up to three genres.
4. Start a 30-minute Deep Work session using Medium unless the answers clearly
   selected Low or High.
5. After the session, request an optional two-part rating: `helped me focus` and
   `I liked the sound`.

Onboarding is stored locally. A new empty database shows it; a migrated profile
with any saved activity, per-activity intensity/timer/genre/mood, feedback, or a
non-default volume is deterministically treated as already onboarded. Global
onboarding genres are an ordered (alphabetical), validated list of at most three
terms from the genre vocabulary and never replace the exact per-activity genre
choice. The final button is the sole action that starts audio and commits the
30-minute Deep Work countdown; an audio or persistence failure leaves onboarding
visible and retryable.

Effectiveness and enjoyment are separate signals. A pleasant track is not
necessarily effective, and an effective track need not be a favourite.
For each installed, validated item and selected activity, the player presents
two optional local questions: focus effectiveness (`helps focus` or
`distracting`, with Clear for unset) and sound enjoyment (`liked` or `not for
me`, with Clear for unset). Enjoyment never changes focus-playback eligibility
or ranking.

### Returning use

The home screen shows:

- Five activity cards
- Last-used sound profile
- Stimulation selector
- Play button
- Compact session-type selector

The app remembers the last combination per activity. Starting the app must not
open an Explore feed or require choosing a track.

### Player

The player provides play/pause, previous/next, volume, timer, activity, sound
profile, stimulation level, favourite, `less like this`, and session rating.
Track details and advanced controls are collapsed by default. During a playing
or paused session, a clearly labelled `Enter focus view` control opens a
distraction-free view. It renders only the current activity, interval work or
break phase where applicable, a truthful large time display, Pause or Resume,
and `Exit focus view`. Countdown and interval sessions display remaining time;
infinite sessions display elapsed focus time labelled `Focused`. Escape exits
this view without changing playback. The view closes automatically when the
session stops or expires.

## Personalisation

Version 1 uses a transparent local scoring model:

```text
score = activity_match
      + genre_preference
      + mood_preference
      + effectiveness_history
      + freshness
      - recent_repetition
      - disliked_audio_features
```

The model must never infer an ADHD diagnosis. Onboarding answers and a chosen
High setting are preferences only. All ratings and derived preferences remain
local unless cloud synchronisation is added with explicit consent.

## Recent sessions and optional ratings

After audio has actually started, the app keeps a local session record with the
chosen activity, stimulation setting, timer configuration, and truthful active
focus time. Stopped and expired sessions may be rated independently as
`helped_focus`, `neutral`, or `distracting`, and `liked` or `not_for_me`; either
answer may be cleared. These are session notes, not track feedback, and never
change music selection. A session found active after an app restart is recorded
as `interrupted` without a fabricated focus duration.

## Catalogue and offline behaviour

Every content item records its licence/provenance, generation model or composer,
prompt/version where applicable, genre, mood, activity suitability, brightness,
onset density, loudness, dynamic range, stimulation variants, seamless regions,
and human-QA status.

The Windows application ships with a small starter library and supports optional
content packs. Installed content must work with no network connection. The app
must not scrape, download, or call Brain.fm.

## Version 1 scope

- Windows installer and signed application when signing credentials are available
- Cross-platform UI and domain foundation
- Five Focus activities
- Metadata-driven genres and moods
- Off, Low, Medium, and High stimulation levels
- Infinite, countdown, and interval sessions
- Seamless, gapless playback with gentle track transitions
- Favorites, dislikes, recent sessions, and effectiveness ratings
- Offline starter catalogue and content-pack import
- Local personalisation
- Distraction-free player
- Focused-window transport shortcuts: Space toggles an active session, and delivered
  Media Play/Pause and Media Stop events control active transport. This is not a
  system-wide media-key feature; physical Windows delivery must be verified.

## Explicitly deferred

- Account system and cloud synchronisation
- Streaming subscription service
- Live AI music generation
- Wearable or biometric adaptation
- Relax, Sleep, Meditate, and guided voice content
- Public claims that the DSP improves ADHD symptoms

## Product acceptance criteria

1. A returning user can start the last-used focus profile with one click.
2. Activity, genre, mood, intensity, and timer are independent persisted values.
3. High/ADHD changes DSP intensity without changing master volume.
4. Switching intensity does not stop playback or produce a click.
5. No starter-catalogue track contains lyrics, speech, abrupt silence, clipping,
   or an unreviewed licence.
6. Playback remains continuous across track boundaries and timer transitions.
7. The app remains fully usable offline after installation of a content pack.
8. Effectiveness and enjoyment can be rated independently.
9. The application contains no medical-treatment language.
10. Windows installation, playback, suspend/resume, device switching, and
    uninstall pass the release checks in `docs/verification.md`.
# Master volume

Listeners can set a global 0–100% master volume. It is independent from stimulation intensity and does not change the selected activity, timer, or transport state. Invalid saved values are reported rather than silently repaired.
# Installed-track navigation

The normal player offers Previous and Next for a playing installed multi-track program. Quarantined review, fallback audio, paused/stopped sessions, and distraction-free view do not offer navigation. Media Track Next/Previous work only when delivered to the focused application window.
