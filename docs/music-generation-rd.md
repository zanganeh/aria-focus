# Music-generation R&D protocol

**Date:** 22 July 2026  
**Status:** pilot protocol; not a release gate yet

This document defines how Aria Focus will improve generated instrumental music
without choosing a final model too early. The first experiment changes prompts
while holding the generator, model snapshot, duration, and seeds fixed. A later
experiment can compare models using the winning prompt family.

## What the initial research suggests

### Prompts should be structured, not adjective piles

Current creator guidance converges on the same useful blocks:

- purpose or use case;
- genre and a more specific subgenre;
- mood and energy;
- approximate tempo and meter;
- instrument roles, not only an instrument list;
- texture and production character;
- arrangement or evolution over time; and
- explicit exclusions.

Udio describes prompts using genre, mood, tempo, instruments, and a clear
creative vision, and recommends generating multiple versions while changing
details deliberately. Its later “Brick Method” groups genre, mood,
instrumentation, and the intended shape/use of the piece into a repeatable
order. Stability AI's prompt guide similarly recommends use case, genre,
specific BPM, nuanced mood, instrument roles, texture, production, and
arrangement. These are useful prompting hypotheses, not guarantees for every
model:

- [Udio: Prompt Like a Master](https://help.udio.com/en/articles/10716541-prompt-like-a-master)
- [Udio: The Brick Method](https://help.udio.com/en/articles/12232112-the-brick-method)
- [Stable Audio 2.5 Prompt Guide](https://stability.ai/implementations/stable-audio-25-prompt-guide)

Community feedback points to recurring failure modes: generic or repetitive
results, genre drift during a track, too many competing instructions, and
outputs that sound impressive for a short preview but become tiring in a work
session. We will measure these directly rather than treating prompt length or
model claims as quality evidence.

### Human preference is the primary product signal

MusicGen evaluated both automatic metrics and human studies, but newer work
shows why an automatic score cannot be the product KPI. A 2025 benchmark
generated 6,000 songs and collected 15,000 pairwise comparisons from 2,500
participants. Another study found that standard FAD correlated weakly with
human preference in open-ended text-to-music evaluation; its proposed MAD
metric correlated better, but still does not measure “helpful during focused
work.”

Sources:

- [MusicGen paper](https://arxiv.org/abs/2306.05284)
- [Benchmarking Music Generation Models and Metrics via Human Preference Studies](https://arxiv.org/abs/2506.19085)
- [Aligning Text-to-Music Evaluation with Human Preferences](https://arxiv.org/abs/2503.16669)

Automatic analysis remains valuable for rejecting broken files and measuring
repeatable properties. It must not replace blind listening and task-based
review.

### Model choice is a separate experiment

The current production plan uses ACE-Step as the local baseline. Its official
project supports short tags, descriptive text, and use-case prompts, and
offers controls relevant to this product. That makes it a reasonable baseline,
not a permanent decision. ACE-Step, MusicGen, Stable Audio, or a hosted model
must be compared using the same prompt families, the same acceptance gates,
and the same human review protocol. Do not compare vendor leaderboard numbers
as if they predict Aria Focus usefulness.

- [ACE-Step official repository](https://github.com/ace-step/ACE-Step)
- [AudioCraft/MusicGen official repository](https://github.com/facebookresearch/audiocraft)

## KPI model

The ranking has two layers.

### Layer 1: hard technical gates

A candidate is rejected before subjective ranking if it has any of the
following:

- decode failure, invalid samples, clipping, or a discontinuity;
- unexpected silence, notification-like sound, speech, singing, or vocal
  fragments;
- duration, channel, or sample-rate mismatch;
- a large loudness jump or an unsafe transition; or
- missing generation provenance, licence evidence, or an immutable hash.

These gates answer “can the application safely play this?” They do not answer
“is this good music?”

### Layer 2: blinded human scores

Review each candidate on a 1–7 scale:

| Dimension | Question | Weight |
| --- | --- | ---: |
| Focus utility | Does it help the intended task continue? | 30% |
| Prompt adherence | Does it sound like the requested genre, mood, tempo, and instrumentation? | 20% |
| Musical coherence | Does it evolve naturally without losing its identity? | 15% |
| Pleasantness | Is it enjoyable enough to keep playing? | 15% |
| Low distraction | Does it avoid hooks, surprises, harshness, and attention capture? | 10% |
| Repeat willingness | Would the listener choose it again tomorrow? | 10% |

For a technically passing candidate:

~~~text
focus_score =
    0.30 * focus_utility
  + 0.20 * prompt_adherence
  + 0.15 * musical_coherence
  + 0.15 * pleasantness
  + 0.10 * low_distraction
  + 0.10 * repeat_willingness
~~~

The primary pilot result is not only the weighted score. Record the pairwise
win rate against the current baseline and the reason for every skip. A track
with a high “musical quality” score but a low focus-utility score is not a
successful Aria Focus track.

The pilot is considered promising when a condition has:

- no hard technical failure;
- median focus utility and low distraction of at least 5/7;
- median prompt adherence of at least 5/7;
- no task-breaking event in either reviewer session; and
- a clear pairwise advantage over the baseline.

These are pilot thresholds for prioritisation, not scientific claims or
clinical measures.

## Small controlled experiment

The first pilot uses one activity: **Motivation**. It generates 12 candidates:
three prompt conditions multiplied by four fixed seed families. The production
ledger derives a unique seed for each condition so it can reject accidental
duplicate candidates while preserving the seed family in the candidate ID. The
experiment matrix is
[motivation-prompt-pilot.json](../experiments/music-generation/motivation-prompt-pilot.json).

### Conditions

1. **Baseline:** the current concise/free-form style description.
2. **Structured:** the same intent expressed as ordered genre, mood, tempo,
   instrument-role, texture, arrangement, and use-case blocks.
3. **Structured-plus-focus:** the structured version with explicit low-
   distraction constraints and a slow evolution plan.

Keep the generator and model snapshot fixed across all three conditions. Use a
180-second lossless master, record every seed and parameter, and do not change
more than the prompt condition during this stage.

The protocol is translated into the strict candidate-ledger plan by
tools/music-generation/build_prompt_pilot_plan.py. The checked-in production
plan is
content/plans/motivation-prompt-pilot-v1.json. After the local model snapshot
has been pinned, the reproducible commands are:

~~~powershell
$root = (Resolve-Path .).Path
$plan = 'content/plans/motivation-prompt-pilot-v1.json'
$python = '.local/music-generation/.venv/Scripts/python.exe'
& $python -B tools/music-generation/production.py preflight --root $root --plan $plan
& $python -B tools/music-generation/production.py run --root $root --plan $plan --run-id motivation-prompt-pilot-v1
~~~

The generated masters and their reports remain quarantined under
.local/music-generation/runs/motivation-prompt-pilot-v1; they are not
application content.

### Review procedure

1. Run the mechanical analyzer and reject hard failures.
2. Give each surviving candidate an anonymous ID; hide condition, prompt,
   model, and seed from reviewers.
3. Perform a short listening pass and score prompt adherence, coherence,
   pleasantness, and distraction.
4. Take the best six candidates into a 15-minute matched task. Use “starting a
   task that is easy to postpone” as the Motivation task.
5. Record focus utility, urge to skip, fatigue, task completion, and any
   notification-like or attention-capturing event.
6. Compute the weighted scores, pairwise wins, medians, and skip reasons.
7. Select a prompt family, not a single lucky track. Generate a small second
   batch from that family before publishing anything.

At least two reviewers are required for this pilot. The result is directional
R&D evidence, not a general claim about ADHD, productivity, or music quality.

## Prompt design used by the pilot

The prompt builder should eventually expose these controls in the UI:

~~~text
Purpose: motivation to begin focused work
Genre: electronic / melodic focus groove
Mood: energized, optimistic, determined, uplifting
Tempo: 108 BPM, steady 4/4
Roles: restrained arp as a background pulse; warm synth bass; soft electric keys;
       light electronic percussion; sparse organic accents
Texture: wide processed strings, soft atmospheric pad, clean modern mix
Evolution: neutral intro; stable groove; small timbral changes every 16–32 bars;
           gentle lift without a drop; neutral outro
Constraints: instrumental; no voice, speech, chanting, whispers, lead hook,
             solo, alarm, notification sound, sharp transient, dramatic build,
             breakdown, riser, or sudden silence
Use: background music for starting a focused work session
~~~

The simple UI can collect the high-value fields. The advanced panel can show
the generated positive and negative prompts, the locked safety constraints,
and the exact generation metadata. Users should be able to adjust one block at
a time so that feedback remains attributable to a change.

## What comes after the pilot

1. Repeat the winning prompt family for Deep Work and Learning.
2. Compare model candidates using the same prompt matrix and human protocol.
3. Add model-specific prompt adapters only after the shared prompt schema is
   stable.
4. Add in-app feedback: skip reason, distraction flag, focus rating, and
   replay choice. Store these as personal preference data, not medical data.
5. Only then generate the larger catalogue and consider v1.0.2.

Generated tracks remain candidates until provenance, technical analysis, human
review, and publication approval are all complete.
