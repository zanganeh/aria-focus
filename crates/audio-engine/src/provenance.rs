//! Provenance metadata for the bundled test asset.
//!
//! Every content item records provenance and licence. The bundled test tone is
//! generated in-process from public-domain maths, so it carries no third-party
//! audio, no named third-party product asset, and no generation provider terms.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub asset_id: &'static str,
    pub title: &'static str,
    pub generator: &'static str,
    pub generator_version: &'static str,
    pub source: &'static str,
    pub licence: &'static str,
    pub contains_voice_or_speech: bool,
    pub contains_lyrics: bool,
    pub notes: &'static str,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub duration_seconds: f32,
    pub loops_seamlessly: bool,
}

impl Provenance {
    /// Provenance for the bundled Deep Work test pad.
    pub fn bundled_test_tone(sample_rate: u32, duration_seconds: f32) -> Self {
        Self {
            asset_id: "starter/deep-work-pad-v1",
            title: "Deep Work Pad (bundled test tone)",
            generator: "audio-engine procedural sine-drone generator",
            generator_version: "0.1.0",
            source: "Generated in-app from public-domain additive synthesis. No third-party audio.",
            licence: "No external audio sampled. Released under the project licence (MIT OR Apache-2.0).",
            contains_voice_or_speech: false,
            contains_lyrics: false,
            notes: "Soft additive chord with a slow amplitude LFO. No hooks, drops, or abrupt changes. Intensity processing is generic tremolo/noise mix and is not a reproduction of any named third-party product.",
            sample_rate_hz: sample_rate,
            channels: 2,
            duration_seconds,
            loops_seamlessly: true,
        }
    }
}
