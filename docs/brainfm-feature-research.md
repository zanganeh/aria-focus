# Brain.fm feature and ADHD-mode evidence

Evidence refreshed: 11 July 2026

This note separates observable product behaviour, published experimental
evidence, Brain.fm marketing claims, and our independent product decisions. It
is not a reverse-engineering specification and does not authorize copying
Brain.fm audio, branding, private APIs, or patented implementation details.

## What the current product demonstrably offers

The current Apple and Google store listings establish the following public
feature surface:

- Focus activities include deep work, learning, creativity, and additional
  task-oriented choices. Relax, sleep, and meditation are separate top-level
  purposes.
- Users can browse by Activities, Genres, and Moods. Current store copy names
  examples ranging from Lo-Fi and Classical to Nature soundscapes; the Apple
  release history also describes an Explore tab and search across those three
  facets.
- The stimulation or "neural effect" level is adjustable, with an ADHD boost.
  Brain.fm describes the boost as extra stimulation, not as a separate musical
  genre.
- The service personalizes a starting setting for a reported "brain type",
  supports downloads/offline playback, and includes a Pomodoro mode.
- Playback is presented as a low-decision workflow: choose the intended mental
  state and music preference, then play. Exact catalogue-ranking logic is not
  public.

Primary product sources:
[Google Play listing](https://play.google.com/store/apps/details?id=com.brainfm.app&hl=en-US)
and [Apple App Store listing](https://apps.apple.com/us/app/brain-fm-focus-sleep-music/id1110684238).

The user's recollection—choosing a preferred genre, choosing Motivation, Deep
Work, Creativity, Learning, or Light Work, and enabling an ADHD mode—is
therefore consistent with the public product surface. Public sources do not
prove that every platform, account, or release shows identical labels.

## What "ADHD mode" can and cannot be inferred to mean

Brain.fm publicly equates its ADHD option with its strongest or boosted neural
effect. Its own explanatory material describes this as greater auditory
stimulation. That supports modelling our control as an independent intensity
dimension:

```text
activity × genre preference × mood preference × stimulation intensity
```

It does **not** support treating ADHD as a genre, automatically diagnosing a
listener, or assuming High is best for every person with ADHD. Users need an
instant Off/Low/Medium/High comparison at matched loudness, and their choice
must remain a local preference rather than a clinical label.

## What the peer-reviewed study actually shows

Woods et al., published in *Communications Biology* in October 2024, tested
music with parametrically manipulated amplitude-modulation rate and depth. The
paper reports:

- rapidly modulated music produced greater activity in attentional networks in
  fMRI and greater stimulus–brain coupling in EEG;
- in the parametrically controlled experiment, beta-range modulation helped
  sustained-attention performance more than other modulation ranges for
  participants reporting more ADHD symptoms; and
- effects varied by listener and experimental timing rather than showing a
  universal improvement.

Source: [Rapid modulation in music supports attention in listeners with
attentional difficulties](https://www.nature.com/articles/s42003-024-07026-3).

Important limits:

- This is evidence about specified experimental stimuli and tasks, not proof
  that an arbitrary tremolo effect, our current DSP, or the commercial ADHD
  button provides the same benefit.
- ADHD-symptom scores are not equivalent to a clinical ADHD diagnosis.
- One author was a Brain.fm employee, Brain.fm produced the modulated-music
  conditions, and the work acknowledged NSF STTR funding. These facts do not
  invalidate the paper, but they matter when weighing generality and conflict
  of interest.
- The result does not justify medication-replacement, treatment, or guaranteed
  productivity claims.

## Technology and intellectual-property boundary

The published paper discusses amplitude modulation, rate, depth, spectral
content, arousal, EEG coupling, and neural responses. Brain.fm also has active
patent claims concerning selecting, creating, and serving audio using
modulation characteristics. One relevant granted patent is
[US11966661B2](https://patents.google.com/patent/US11966661B2/en), *Audio content
serving and creation based on modulation characteristics*.

Accordingly, the current generic amplitude/noise processor remains an
experimental, clearly labelled intensity effect. Before public or commercial
release, counsel must perform a claim-level review of the exact DSP and content
pipeline. A patent's existence is not evidence that its claims are scientifically
effective, and a scientific paper is not freedom-to-operate advice.

## Product contract derived from the evidence

Our Focus home screen should preserve the useful workflow without cloning the
competitor:

1. Pick one task intent: Motivation, Deep Work, Creativity, Learning, or Light
   Work.
2. Retain or change a preferred genre and optional mood filter. These are
   independent metadata facets, not five separate copies of the library.
3. Retain or change Off, Low, Medium, or High stimulation. High may be described
   as "strong / ADHD-friendly option" with a variability disclaimer, never as
   treatment.
4. Pick Infinite, Countdown, or Intervals, then play. A returning user starts
   the last profile with one action.
5. During playback, allow immediate matched-loudness intensity switching,
   favourite, less-like-this, and a two-question post-session rating without
   interrupting the session.

Catalogue selection must first enforce technical, provenance, human-QA,
activity, and continuous-playback eligibility. It may then rank by activity,
genre, mood, prior effectiveness, freshness, and repetition. It must never
fabricate a match by relabelling the same generic track across the whole
catalogue.

## Music-quality implications

The published feature set alone does not explain Brain.fm's musical quality.
For our catalogue, "dynamic" means slow, coherent evolution over minutes—not
drops, hooks, fills, sudden silence, sharp brightness changes, or conspicuous
solos. The strongest intensity setting must not simply be louder. Source music
and DSP variants require loudness matching, long-session review, and explicit
fatigue/distraction ratings.

AI generation is therefore only a candidate-production method. ACE-Step output
must pass immutable provenance, decoding and analyzer gates, then two human
reviews including a representative work session before it becomes starter
content. Downloaded music is acceptable only with redistribution rights that
cover bundling it inside the application.
