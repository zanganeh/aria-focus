/**
 * Centralised user-facing copy. Kept here so an automated test can prove the
 * product framing contains no medical-treatment language and no claim that
 * the DSP reproduces any named third-party product.
 */
export const PRODUCT_COPY = {
  disclaimer:
    "A productivity and environmental-support tool for focus work. It uses " +
    "background audio to support attention and is not a health intervention, " +
    "clinical device, or substitute for professional care. Stimulation is " +
    "generic audio processing and does not reproduce any third-party product.",
  intensityNote:
    "Intensity changes the audio processing, not the volume. Individual " +
    "response varies; you can switch or turn it off at any time.",
};

/** Substrings that must never appear in product copy. */
export const BANNED_PHRASES = [
  "treat",
  "cure",
  "diagnos",
  "therapy",
  "neurofeedback",
  "medication",
  "prescription",
];
