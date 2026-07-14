//! Audio engine core for the ADHD Music focus player.
//!
//! Native CPAL output accepts fully decoded installed WAV/FLAC/MP3 programs or
//! an explicit licence-clean procedural fallback. Whole-file decode, one-time
//! resampling, and device mapping happen before the artifact-smoothed realtime
//! callback behind an atomic control boundary.
//!
//! IMPORTANT: the stimulation processor uses generic, well-known amplitude
//! tremolo and noise-mixing techniques. It does NOT reproduce Brain.fm and
//! makes no medical or patent claim. Public/commercial use of neural-style
//! stimulation DSP requires a claim-level patent review before release.

mod backend;

pub mod dsp;
pub mod media;
pub mod playback;
pub mod provenance;
pub mod source;
pub mod tone;

pub use dsp::{IntensityProfile, Processor, Waveform};
pub use media::{
    adapt_program_for_device, decode_generated_draft_flac, decode_track, decode_track_with_limit,
    AuthoredRegion, AuthoredRegionKind, DecodeExpectation, DecodedProgram, DecodedTrack,
    DeviceProgram, DeviceTrack, MediaCodec, MediaError, SourceLabel, MAX_DEVICE_PROGRAM_SAMPLES,
    MAX_PROGRAM_SAMPLES,
};
pub use playback::{AudioError, AudioFacade, AudioIntensity, NativeAudioFacade, PlaybackState};
pub use provenance::Provenance;
pub use source::{PlaybackSource, PlaybackSourceKind, ProgramRenderer};
pub use tone::generate_test_tone;

/// Stereo interleaved PCM at 32-bit float, the internal render format.
pub type Pcm = Vec<f32>;

/// Default sample rate used by deterministic source and DSP tests.
pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;
