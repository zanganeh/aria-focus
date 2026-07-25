# Music prompt system

**Status:** working v1 taxonomy and deterministic prompt builder  
**Purpose:** turn structured musical metadata into repeatable, reviewable
instrumental-generation prompts.

The system keeps the metadata separate from the wording sent to a generation
model. A profile describes the musical target; the prompt builder translates
that profile into instrument roles, arrangement behaviour, and explicit safety
constraints. This makes prompt changes attributable and lets us compare model
outputs fairly.

## Catalog

The current sanitized catalog contains 40 co-occurrence profiles. It retains
musical descriptors and removes names, opaque IDs, artwork URLs, audio URLs,
authentication, and cookies.

The complete machine-readable catalog is
[content/music/prompt-catalog-v1.json](../content/music/prompt-catalog-v1.json).

### Activities

Creative Flow, Creativity, Deep Work, Learning, Light Work, Motivation,
Sleep And Wake, Study & Read.

### Genres

The observed music genres are Cinematic, Electronic, Grooves, Lofi, and Piano.
The wider selector vocabulary also includes Atmospheric, Beach, Chimes & Bowls,
Forest, Nightsounds, Rain, Rainforest, River, Thunder, Underwater, and Wind.

### Subgenres

Acoustic, Ambient, Classical, Dance, Disco, Downtempo, Drum and Bass,
Electronica, Funk, Hip-Hop, House, Jazz, Jungle, Lo-Fi, Orchestral, Post-Rock,
R&B, Reggae, Rock, Soul, Techno, Trance, and World/Ethnic.

### Moods

Brooding, Dark, Downtempo, Driving, Energizing, Epic, Heavy, Hopeful,
Inspiring, Mysterious, Ominous, Optimistic, Playful, Ponderous, Strong, Upbeat,
and Uplifting.

### Instruments

Acoustic Bass, Acoustic Drum set, Acoustic Drumset, Acoustic Piano, Arp Synth,
Arp Synth Bass, Chimes/Bells, Choral Voices, Electric Bass, Electric Guitar,
Electric Keys, Electronic Percussion, Ethnic Percussion, Ethnic Strings, Horns,
Mallets, Orchestral Brass, Orchestral Percussion, Orchestral Strings,
Orchestral Winds, Organic Percussion, Pedal Steel, Processed Strings, Processed
Vocals, Synth Bass, Synth Pad, and Textural Soundscape.

The duplicate labels `Acoustic Drum set` and `Acoustic Drumset` should be
normalised to the stable ID `acoustic-drumset`. Voice-like labels are retained
for analysis but excluded from instrumental role selection by default.

### Numeric controls

| Field | Meaning | Observed range |
| --- | --- | ---: |
| BPM | pulse speed | 83–170 |
| Brightness | spectral brightness, 0 dark to 1 bright | 0.20–0.89 |
| Complexity | density and activity, 0 sparse to 1 dense | 0.04–0.98 |
| Neural effect | optional effect-strength descriptor | 0.02–1.00 |

Brightness and complexity are continuous controls. They should be mapped to
plain language only at prompt time: dark/warm, balanced, or bright/open;
sparse, moderately layered, or densely evolving.

## Prompt construction

The builder uses this order so the model receives a coherent musical brief:

1. **Purpose:** activity and listening context, such as motivation for starting
   focused work.
2. **Identity:** one primary genre followed by specific subgenres.
3. **Emotion:** the selected mood set, reduced if it becomes contradictory.
4. **Pulse:** BPM and meter.
5. **Roles:** instruments become performance roles instead of a bare list. For
   example, an arp is a restrained background pulse, strings carry evolving
   harmony, and percussion provides controlled forward motion.
6. **Tone:** brightness, complexity, register, depth, and mix character.
7. **Evolution:** an introduction shorter than 10 seconds, a quickly established
   pulse, 16–32 bar changes in harmony/register/texture, a controlled lift, and
   a resolved crossfade-friendly ending.
8. **Negative constraints:** instrumental-only rules, no speech or singing,
   no piercing or toy-like timbres, no sudden silence, and no unchanged short
   loop repetition.

The positive prompt should describe what to perform. The negative prompt
should describe what must not appear. Avoid mixing contradictory instructions
such as asking for a bright bell lead while also forbidding bright high
frequency material.

## Profile selection

When a user chooses an activity and genre, select profiles in this order:

1. exact activity match;
2. exact primary genre match;
3. maximum mood overlap;
4. maximum instrument-role overlap;
5. nearest BPM and brightness/complexity targets; and
6. a diversity penalty when the profile was recently played.

Keep the selected profile ID, normalized metadata, prompt version, seed, model
configuration, and final prompt beside every generated candidate. This allows
listening feedback to improve the selector and the prompt template separately.

## Example

The first profile produces a Motivation/Electronic example at 116 BPM with
Dance, Electronica, and House subgenres. Its source metadata includes a
voice-like instrument label, so that label is recorded as excluded and is not
used as a positive instrumental role.

The generated example is stored at
[content/music/examples/motivation-electronic-profile-001.json](../content/music/examples/motivation-electronic-profile-001.json).

Its central positive direction is:

> Create a 3-minute premium instrumental track for motivation and sustained
> focus. Use a restrained arpeggiated pulse, rounded electric keys, controlled
> electronic percussion, sparse organic percussion, evolving processed
> strings, warm low-mid synth support, and a soft atmospheric pad. Establish
> the pulse in under 10 seconds, develop 16-to-32-bar variations, and finish
> with a resolved crossfade-friendly outro.

The negative prompt additionally excludes vocals, speech, jazz, swing, blues,
funk, piercing high frequencies, sub-bass rumble, toy instruments, dramatic
risers, breakdowns, and 10-second loop repetition for this particular
non-jazz focus profile.

## Rebuilding the catalog

Use the extractor whenever a new sanitized session export is available:

```powershell
python -B tools/music-generation/derive_prompt_catalog.py `
  --har "C:\path\to\session.har" `
  --output content/music/prompt-catalog-v1.json `
  --example-output content/music/examples/motivation-electronic-profile-001.json `
  --profile-id profile-001 `
  --activity Motivation
```

The extractor is intentionally closed-world: it reads only the response JSON,
keeps the taxonomy and profile co-occurrence, and never copies URLs, headers,
cookies, artwork, or audio references into the repository. Keep raw session
exports outside the repository and treat them as private credentials-bearing
files.

## Review gate for generated music

Before a candidate becomes application content, verify:

- the file decodes and has the expected duration and channels;
- it contains no speech, singing, vocal fragments, notification-like sounds,
  or unsafe loudness transitions;
- the first musical pulse arrives within the requested intro limit;
- the arrangement changes meaningfully without a repeated short loop;
- the result matches the selected genre, mood, instruments, BPM, and tone; and
- the prompt, profile, model settings, seed, analysis, and hash are preserved.

The catalog is a prompt-design input, not a claim that every generated result
will be good. Human listening remains the final quality signal.
