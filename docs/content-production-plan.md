# Starter music production plan

Date: 10 July 2026

Status: provider research and production contract approved for candidate work;
no generated candidate is release content until technical and human gates pass.

## Decision

Use ACE-Step 1.5 locally as the primary candidate generator. Pin the repository
commit, model identifiers, and downloaded weight hashes before the first batch.
Generate lossless 48 kHz FLAC masters with explicit seeds. Keep generation out
of the application and out of the real-time playback path.

Use Lyria 3 Pro only as an optional paid quality benchmark if API billing is
made available. Do not make Eleven Music or Stable Audio a release dependency
until the exact commercial terms for distributing a music library inside this
application have been reviewed and archived.

This is a content-production decision, not a claim that one generator is always
musically superior. Listener review can replace any candidate or provider.

## Primary-source findings

### ACE-Step 1.5

- The official model card identifies the code and model as MIT licensed and
  describes training data as licensed, royalty-free/public-domain, and
  synthetic. It explicitly presents generated music as commercially usable.
- It supports deterministic seeds, 10–600 second duration, BPM, key, time
  signature, instrumental structure, 48 kHz output, and lossless FLAC.
- Official guidance recommends the 2B Turbo or SFT model with a 0.6B or 1.7B
  language model for 8–16 GB VRAM. This development machine has an NVIDIA RTX
  3060 with 12 GB VRAM, so the 2B model and 0.6B language model are the
  conservative first configuration.
- The installation requires Python 3.11–3.12 and roughly 10 GB for core models.

Sources: [official repository](https://github.com/ace-step/ACE-Step-1.5),
[official model card](https://huggingface.co/ACE-Step/Ace-Step1.5),
[inference reference](https://github.com/ace-step/ACE-Step-1.5/blob/main/docs/en/INFERENCE.md),
and [MIT licence](https://github.com/ace-step/ACE-Step-1.5/blob/main/LICENSE).

MIT licensing and a provider statement do not themselves guarantee copyright
subsistence, exclusivity, or non-infringement in every jurisdiction. Preserve
the exact model/terms snapshot and obtain legal review before commercial
release.

### Google Lyria 3

- Lyria 3 Clip produces fixed 30-second MP3 clips. Lyria 3 Pro produces
  couple-of-minute 44.1 kHz stereo pieces and can return WAV.
- It supports instrumental prompts, BPM/key/structure instructions, and embeds
  an imperceptible SynthID watermark.
- It is currently a preview model, single-turn, and non-deterministic. Google
  does not claim ownership of generated content, but the developer remains
  responsible for lawful use. Paid API data is not used to improve Google's
  products under the current additional terms.

Sources: [Lyria 3 generation documentation](https://ai.google.dev/gemini-api/docs/music-generation),
[model card](https://deepmind.google/models/model-cards/lyria-3/), and
[Gemini API terms](https://ai.google.dev/gemini-api/terms).

### Eleven Music

- The API supports instrumental generation, section-level positive and
  negative directions, 3-second to 10-minute duration, and up to 44.1 kHz
  output. Inpainting is currently an enterprise feature.
- ElevenLabs states that training uses licensed stems and that paid-plan output
  has broad commercial rights, but rights vary by plan and use case. A music
  library distributed in an app is not assumed covered merely because general
  commercial use is advertised.

Sources: [official Music API overview](https://elevenlabs.io/music-api),
[music documentation](https://elevenlabs.io/docs/overview/capabilities/music),
and [API terms](https://elevenlabs.io/music-api-terms).

### Stable Audio

Stable Audio remains a possible secondary local generator. The Stability AI
Community License permits commercial core-model use below USD 1 million annual
revenue, with registration and enterprise-licence implications at or above the
threshold. That moving business constraint makes it less suitable than the MIT
licensed ACE-Step baseline for this project.

Source: [Stability AI licence](https://stability.ai/license).

## Starter catalogue target

Produce at least 20 candidates and accept at most 10 initial tracks:

| Activity | Initial sound families | Candidate target | Acceptance target |
| --- | --- | ---: | ---: |
| Deep Work | ambient electronic, restrained minimal pulse | 4 | 2 |
| Motivation | steady electronic groove, light percussion | 4 | 2 |
| Creativity | organic-electronic, warm textural movement | 4 | 2 |
| Learning | low-density ambient, soft tonal pattern | 4 | 2 |
| Light Work | mellow lo-fi/electronic, gentle rhythmic bed | 4 | 2 |

Genres and moods remain metadata. A piece may suit multiple activities, but the
release must not manufacture variety by relabelling one generic asset as the
whole catalogue.

Target candidate duration is 180 seconds. Prefer 70–100 BPM, stable 4/4 meter,
restrained melodic range, stable instrumentation, and slow timbral change.
Generation prompts must explicitly exclude voice, speech, vocal samples,
chanting, whispers, lead hooks, solos, fills, drops, breakdowns, stingers,
dramatic builds, sudden silence, vinyl crackle, alarms, and notification-like
tones.

## Immutable generation record

Record this before any editing:

- candidate ID and intended activity, genre, and mood;
- generator, repository commit, model/LM identifiers, weight hashes, and
  relevant terms snapshot hash;
- complete positive and negative prompts or structured caption;
- seed, duration, BPM, key, meter, sampler, guidance, inference steps, and all
  non-default parameters;
- generation timestamp, machine/GPU, original filename, byte length, codec,
  sample rate, channels, and SHA-256;
- any provider response ID or watermark status; and
- every subsequent edit as a derived asset with tool/version, command or
  settings, parent SHA-256, and output SHA-256.

Do not label generated output `CC0` unless a rights holder actually applies
CC0. Use an archived, dated provider-output terms identifier and URL, with the
generator/model recorded separately.

## Mechanical rejection gates

Analyze the untouched master and the final edited asset. Reject automatically
on decode failure, non-finite samples, clipping, unexplained silence, channel
or duration mismatch, discontinuity, missing provenance, or missing licence
evidence. Flag rather than silently accept:

- detected speech, singing, vocalisation, or notification-like events;
- unstable tempo or high tempo uncertainty;
- excessive onset density or a large within-track onset change;
- high section-change novelty, loudness jumps, or spectral surprises;
- unsuitable true peak, integrated loudness, or loudness range; and
- no technically safe loop or two-sided crossfade region.

Numeric thresholds remain calibration values until they have been compared
with accepted and rejected listener examples. The analyzer must emit raw
measurements and reasons, not only a pass/fail bit.

The Phase 1C2a adjacent-sample jump detector emits provisional discontinuity
candidates, not confirmed defects. Those candidates are flags until waveform
inspection or listening confirms the discontinuity required by the hard gate.

## Human acceptance

Every accepted track requires two distinct reviewers. At least one review must
cover a representative 45–90 minute work session using the actual application.
Reviewers score:

- technical defects and transition quality;
- distraction, urge to skip, fatigue, and notification-like salience;
- musical coherence and whether evolution is too static or too attention
  grabbing;
- suitability for each of the five activities; and
- Off, Low, Medium, and High stimulation comfort.

Any speech, hook, sharp transition, unresolved provenance issue, or reviewer
release blocker keeps the item out of a published pack. AI-generated or
automated review cannot count as either human reviewer.

## Next implementation tranche

1. Calibrate the reproducible offline analyzer documented in
   `docs/audio-analyzer.md` against accepted and rejected full-length candidates;
   its current thresholds remain provisional.
2. Add a candidate ledger and immutable generation-record schema with hashes.
3. Pin and install ACE-Step 1.5 outside the shipped application, then generate
   one Deep Work calibration batch.
4. Analyze and audition the batch; use accepted/rejected examples to calibrate
   thresholds before generating all five activities.
5. Edit safe regions non-destructively, build a development pack, and run the
   representative-session review protocol.
