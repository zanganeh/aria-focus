//! Native playback facade and allocation-free real-time renderer.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use crate::backend::{CpalThreadOutput, OutputBackend};
use crate::dsp::domain_shim::Intensity as DspIntensity;
use crate::media::SourceLabel;
use crate::source::{PlaybackSource, PlaybackSourceKind, RealtimeSource};
use crate::{IntensityProfile, Processor};

const TRANSPORT_RAMP_SECONDS: f32 = 0.03;
const MASTER_VOLUME_RAMP_SECONDS: f32 = 0.015;
const TRANSPORT_SILENCE_EPSILON: f32 = 1.0e-4;
pub(crate) const MAX_OUTPUT_CHANNELS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioIntensity {
    Off = 0,
    Low = 1,
    Medium = 2,
    High = 3,
}

impl AudioIntensity {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::Low,
            2 => Self::Medium,
            3 => Self::High,
            _ => Self::Medium,
        }
    }

    fn profile(self) -> IntensityProfile {
        let intensity = match self {
            Self::Off => DspIntensity::Off,
            Self::Low => DspIntensity::Low,
            Self::Medium => DspIntensity::Medium,
            Self::High => DspIntensity::High,
        };
        IntensityProfile::for_intensity(intensity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("no default audio output device is available")]
    NoOutputDevice,
    #[error("failed to query the default audio output configuration: {0}")]
    DefaultConfig(String),
    #[error("unsupported audio output sample format: {0}")]
    UnsupportedSampleFormat(String),
    #[error("failed to build the native audio stream: {0}")]
    BuildStream(String),
    #[error("failed to start the native audio stream: {0}")]
    PlayStream(String),
    #[error("the native audio stream reported a device error")]
    StreamFailed,
    #[error("the native audio control thread stopped unexpectedly")]
    ControlThreadStopped,
    #[error("audio transport cannot {operation} while {state:?}")]
    InvalidTransition {
        operation: &'static str,
        state: PlaybackState,
    },
    #[error("injected audio failure")]
    InjectedFailure,
    #[error("installed media could not be prepared: {0}")]
    Media(String),
    #[error("track navigation is unavailable")]
    NavigationUnavailable,
}

/// Control surface used by the Tauri coordinator and its deterministic mocks.
pub trait AudioFacade: Send {
    fn start(&mut self, intensity: AudioIntensity) -> Result<(), AudioError>;
    fn start_with_source(
        &mut self,
        source: PlaybackSource,
        intensity: AudioIntensity,
    ) -> Result<(), AudioError> {
        let _ = source;
        self.start(intensity)
    }
    fn pause(&mut self) -> Result<(), AudioError>;
    fn resume(&mut self) -> Result<(), AudioError>;
    fn stop(&mut self) -> Result<(), AudioError>;
    fn set_intensity(&mut self, intensity: AudioIntensity) -> Result<(), AudioError>;
    fn set_master_volume(&mut self, percent: u8) -> Result<(), AudioError>;
    fn master_volume(&self) -> u8;
    fn state(&self) -> PlaybackState;
    fn intensity(&self) -> AudioIntensity;
    fn source_label(&self) -> SourceLabel {
        SourceLabel::test_fallback()
    }
    fn source_kind(&self) -> PlaybackSourceKind {
        PlaybackSourceKind::TestTone
    }
    fn navigate_next(&mut self) -> Result<(), AudioError> {
        Err(AudioError::NavigationUnavailable)
    }
    fn navigate_previous(&mut self) -> Result<(), AudioError> {
        Err(AudioError::NavigationUnavailable)
    }
    fn navigation_available(&self) -> bool {
        false
    }
}

pub(crate) struct RealtimeControl {
    intensity: AtomicU8,
    target_gain_bits: AtomicU32,
    master_gain_bits: AtomicU32,
    current_track: AtomicU8,
    navigation_target: AtomicU8,
    navigation_generation: AtomicU32,
    navigation_active: AtomicBool,
    stream_failed: AtomicBool,
}

impl RealtimeControl {
    pub(crate) fn new() -> Self {
        Self {
            intensity: AtomicU8::new(AudioIntensity::Medium as u8),
            target_gain_bits: AtomicU32::new(0.0f32.to_bits()),
            master_gain_bits: AtomicU32::new(0.7f32.to_bits()),
            current_track: AtomicU8::new(0),
            navigation_target: AtomicU8::new(u8::MAX),
            navigation_generation: AtomicU32::new(0),
            navigation_active: AtomicBool::new(false),
            stream_failed: AtomicBool::new(false),
        }
    }

    fn set_intensity(&self, intensity: AudioIntensity) {
        self.intensity.store(intensity as u8, Ordering::Release);
    }

    fn intensity(&self) -> AudioIntensity {
        AudioIntensity::from_u8(self.intensity.load(Ordering::Acquire))
    }

    fn set_target_gain(&self, gain: f32) {
        self.target_gain_bits
            .store(gain.to_bits(), Ordering::Release);
    }

    fn target_gain(&self) -> f32 {
        f32::from_bits(self.target_gain_bits.load(Ordering::Acquire))
    }

    fn set_master_volume(&self, percent: u8) {
        self.master_gain_bits
            .store((f32::from(percent) / 100.0).to_bits(), Ordering::Release);
    }
    fn master_gain(&self) -> f32 {
        f32::from_bits(self.master_gain_bits.load(Ordering::Acquire))
    }

    fn set_current_track(&self, track: usize) {
        self.current_track.store(track as u8, Ordering::Release);
    }

    fn current_track(&self) -> usize {
        usize::from(self.current_track.load(Ordering::Acquire))
    }

    fn request_navigation(&self, target: usize) {
        // Target is published before its generation; the renderer reads a stable generation
        // and explicitly rejects the u8::MAX sentinel and every out-of-program value.
        self.navigation_target
            .store(target as u8, Ordering::Release);
        self.navigation_active.store(true, Ordering::Release);
        self.navigation_generation.fetch_add(1, Ordering::AcqRel);
    }

    fn clear_navigation(&self) {
        self.navigation_target.store(u8::MAX, Ordering::Release);
        self.navigation_generation.fetch_add(1, Ordering::AcqRel);
        self.navigation_active.store(false, Ordering::Release);
    }

    pub(crate) fn set_navigation_active(&self, active: bool) {
        self.navigation_active.store(active, Ordering::Release);
    }

    pub(crate) fn mark_stream_failed(&self) {
        self.stream_failed.store(true, Ordering::Release);
    }

    fn check_health(&self) -> Result<(), AudioError> {
        if self.stream_failed.load(Ordering::Acquire) {
            Err(AudioError::StreamFailed)
        } else {
            Ok(())
        }
    }
}

pub(crate) struct RealtimeRenderer {
    control: Arc<RealtimeControl>,
    source: RealtimeSource,
    processors: Vec<Processor>,
    intensity: AudioIntensity,
    gain: f32,
    target_gain: f32,
    gain_coeff: f32,
    master_gain: f32,
    target_master_gain: f32,
    master_gain_coeff: f32,
    navigation_generation: u32,
}

impl RealtimeRenderer {
    pub(crate) fn new(
        sample_rate: u32,
        channels: usize,
        control: Arc<RealtimeControl>,
        source: RealtimeSource,
    ) -> Self {
        let intensity = control.intensity();
        let master_gain = control.master_gain();
        let gain_coeff = 1.0 - (-1.0 / (TRANSPORT_RAMP_SECONDS * sample_rate as f32)).exp();
        let master_gain_coeff =
            1.0 - (-1.0 / (MASTER_VOLUME_RAMP_SECONDS * sample_rate as f32)).exp();
        Self {
            control,
            source,
            processors: (0..channels)
                .map(|_| Processor::new(sample_rate, intensity.profile()))
                .collect(),
            intensity,
            gain: 0.0,
            target_gain: 0.0,
            gain_coeff,
            master_gain,
            target_master_gain: master_gain,
            master_gain_coeff,
            navigation_generation: 0,
        }
    }

    /// Synchronise fixed-size atomic parameters once per output callback.
    pub(crate) fn sync_controls(&mut self) {
        let intensity = self.control.intensity();
        if intensity != self.intensity {
            for processor in &mut self.processors {
                processor.set_target(intensity.profile());
            }
            self.intensity = intensity;
        }
        self.target_gain = self.control.target_gain();
        self.target_master_gain = self.control.master_gain();
        let generation = self.control.navigation_generation.load(Ordering::Acquire);
        if generation != self.navigation_generation {
            self.navigation_generation = generation;
            let target = usize::from(self.control.navigation_target.load(Ordering::Acquire));
            if target != usize::from(u8::MAX) {
                self.source.request_navigation(target, &self.control);
            }
        }
    }

    /// Render one frame. No allocation, locks, I/O, logging, or callbacks.
    #[inline]
    pub(crate) fn next_frame(&mut self, frame: &mut [f32]) {
        self.gain += self.gain_coeff * (self.target_gain - self.gain);
        self.master_gain += self.master_gain_coeff * (self.target_master_gain - self.master_gain);
        if self.target_master_gain == 0.0 && self.master_gain <= TRANSPORT_SILENCE_EPSILON {
            self.master_gain = 0.0;
        }
        if self.target_gain == 0.0 && self.gain <= TRANSPORT_SILENCE_EPSILON {
            self.gain = 0.0;
            frame.fill(0.0);
            return;
        }
        self.source.begin_frame();
        for (channel, (sample, processor)) in
            frame.iter_mut().zip(self.processors.iter_mut()).enumerate()
        {
            let dry = self.source.sample(channel);
            *sample =
                (processor.process_sample(dry) * self.gain * self.master_gain).clamp(-1.0, 1.0);
        }
        self.control.set_current_track(self.source.end_frame());
        self.control
            .navigation_active
            .store(self.source.navigation_active(), Ordering::Release);
    }

    #[cfg(test)]
    fn current_depth(&self) -> f32 {
        self.processors[0].current_depth()
    }
}

struct AudioController<O: OutputBackend> {
    output: O,
    control: Arc<RealtimeControl>,
    state: PlaybackState,
    intensity: AudioIntensity,
    master_volume: u8,
    source_labels: Vec<SourceLabel>,
    source_kind: PlaybackSourceKind,
    track_count: usize,
}

impl<O: OutputBackend + Send> AudioController<O> {
    fn new(output: O) -> Self {
        Self {
            output,
            control: Arc::new(RealtimeControl::new()),
            state: PlaybackState::Stopped,
            intensity: AudioIntensity::Medium,
            master_volume: 70,
            source_labels: vec![SourceLabel::test_fallback()],
            source_kind: PlaybackSourceKind::TestTone,
            track_count: 1,
        }
    }

    fn transition_error(&self, operation: &'static str) -> AudioError {
        AudioError::InvalidTransition {
            operation,
            state: self.state,
        }
    }
}

impl<O: OutputBackend + Send> AudioFacade for AudioController<O> {
    fn start(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
        self.start_with_source(PlaybackSource::TestTone, intensity)
    }

    fn start_with_source(
        &mut self,
        source: PlaybackSource,
        intensity: AudioIntensity,
    ) -> Result<(), AudioError> {
        if self.state != PlaybackState::Stopped {
            return Err(self.transition_error("start"));
        }
        self.control.check_health()?;

        let previous = self.control.intensity();
        self.control.set_intensity(intensity);
        let labels = source.labels();
        let source_kind = source.kind();
        let track_count = labels.len();
        let previous_track = self.control.current_track();
        self.control.clear_navigation();
        self.control.set_current_track(0);
        if let Err(error) = self
            .output
            .ensure_started(Arc::clone(&self.control), source)
        {
            self.control.set_intensity(previous);
            self.control.set_current_track(previous_track);
            return Err(error);
        }

        self.control.set_target_gain(1.0);
        self.control.clear_navigation();
        self.intensity = intensity;
        self.source_labels = labels;
        self.source_kind = source_kind;
        self.track_count = track_count;
        self.state = PlaybackState::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if self.state != PlaybackState::Playing {
            return Err(self.transition_error("pause"));
        }
        self.control.check_health()?;
        self.control.set_target_gain(0.0);
        self.control.clear_navigation();
        self.state = PlaybackState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        if self.state != PlaybackState::Paused {
            return Err(self.transition_error("resume"));
        }
        self.control.check_health()?;
        self.control.set_target_gain(1.0);
        self.state = PlaybackState::Playing;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return Err(self.transition_error("stop"));
        }
        self.control.check_health()?;
        self.control.set_target_gain(0.0);
        self.control.clear_navigation();
        self.state = PlaybackState::Stopped;
        Ok(())
    }

    fn set_intensity(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
        self.control.check_health()?;
        self.control.set_intensity(intensity);
        self.intensity = intensity;
        Ok(())
    }

    fn set_master_volume(&mut self, percent: u8) -> Result<(), AudioError> {
        if percent > 100 {
            return Err(AudioError::Media(
                "master volume must be between 0 and 100".to_owned(),
            ));
        }
        self.control.check_health()?;
        self.control.set_master_volume(percent);
        self.master_volume = percent;
        Ok(())
    }
    fn master_volume(&self) -> u8 {
        self.master_volume
    }

    fn state(&self) -> PlaybackState {
        self.state
    }

    fn intensity(&self) -> AudioIntensity {
        self.intensity
    }

    fn source_label(&self) -> SourceLabel {
        self.source_labels
            .get(self.control.current_track())
            .unwrap_or(&self.source_labels[0])
            .clone()
    }

    fn source_kind(&self) -> PlaybackSourceKind {
        self.source_kind
    }

    fn navigate_next(&mut self) -> Result<(), AudioError> {
        self.navigate(1)
    }
    fn navigate_previous(&mut self) -> Result<(), AudioError> {
        self.navigate(-1)
    }
    fn navigation_available(&self) -> bool {
        self.state == PlaybackState::Playing
            && self.source_kind == PlaybackSourceKind::Installed
            && self.track_count > 1
            && !self.control.navigation_active.load(Ordering::Acquire)
    }
}

impl<O: OutputBackend + Send> AudioController<O> {
    fn navigate(&mut self, direction: isize) -> Result<(), AudioError> {
        if !self.navigation_available() {
            return Err(AudioError::NavigationUnavailable);
        }
        let current = self.control.current_track();
        if current >= self.track_count {
            return Err(AudioError::NavigationUnavailable);
        }
        let target = (current as isize + direction).rem_euclid(self.track_count as isize) as usize;
        if target == current {
            return Err(AudioError::NavigationUnavailable);
        }
        self.control.request_navigation(target);
        Ok(())
    }
}

/// Production facade backed by the platform's CPAL default output device.
pub struct NativeAudioFacade(AudioController<CpalThreadOutput>);

impl NativeAudioFacade {
    pub fn new() -> Self {
        Self(AudioController::new(CpalThreadOutput::new()))
    }
}

impl Default for NativeAudioFacade {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioFacade for NativeAudioFacade {
    fn start(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
        self.0.start(intensity)
    }

    fn start_with_source(
        &mut self,
        source: PlaybackSource,
        intensity: AudioIntensity,
    ) -> Result<(), AudioError> {
        self.0.start_with_source(source, intensity)
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.0.pause()
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.0.resume()
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.0.stop()
    }

    fn set_intensity(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
        self.0.set_intensity(intensity)
    }
    fn set_master_volume(&mut self, percent: u8) -> Result<(), AudioError> {
        self.0.set_master_volume(percent)
    }
    fn master_volume(&self) -> u8 {
        self.0.master_volume()
    }

    fn state(&self) -> PlaybackState {
        self.0.state()
    }

    fn intensity(&self) -> AudioIntensity {
        self.0.intensity()
    }

    fn source_label(&self) -> SourceLabel {
        self.0.source_label()
    }

    fn source_kind(&self) -> PlaybackSourceKind {
        self.0.source_kind()
    }
    fn navigate_next(&mut self) -> Result<(), AudioError> {
        self.0.navigate_next()
    }
    fn navigate_previous(&mut self) -> Result<(), AudioError> {
        self.0.navigate_previous()
    }
    fn navigation_available(&self) -> bool {
        self.0.navigation_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{
        AuthoredRegion, AuthoredRegionKind, DecodedProgram, DecodedTrack, DeviceProgram,
        DeviceTrack,
    };

    #[derive(Default)]
    struct FakeOutput {
        starts: usize,
        fail_start: bool,
    }

    impl OutputBackend for FakeOutput {
        fn ensure_started(
            &mut self,
            _control: Arc<RealtimeControl>,
            _source: PlaybackSource,
        ) -> Result<(), AudioError> {
            if self.fail_start {
                return Err(AudioError::InjectedFailure);
            }
            self.starts += 1;
            Ok(())
        }
    }

    #[test]
    fn audio_state_transitions_are_deterministic_without_a_device() {
        let mut audio = AudioController::new(FakeOutput::default());
        assert_eq!(audio.state(), PlaybackState::Stopped);
        audio.start(AudioIntensity::Medium).unwrap();
        assert_eq!(audio.state(), PlaybackState::Playing);
        audio.pause().unwrap();
        assert_eq!(audio.state(), PlaybackState::Paused);
        audio.resume().unwrap();
        assert_eq!(audio.state(), PlaybackState::Playing);
        audio.stop().unwrap();
        assert_eq!(audio.state(), PlaybackState::Stopped);
        assert!(audio.pause().is_err());
    }

    #[test]
    fn failed_output_start_does_not_commit_audio_state() {
        let mut audio = AudioController::new(FakeOutput {
            starts: 0,
            fail_start: true,
        });
        let result = audio.start(AudioIntensity::High);
        assert_eq!(result, Err(AudioError::InjectedFailure));
        assert_eq!(audio.state(), PlaybackState::Stopped);
        assert_eq!(audio.intensity(), AudioIntensity::Medium);
        assert_eq!(audio.control.intensity(), AudioIntensity::Medium);
    }

    #[test]
    fn realtime_renderer_smooths_atomic_parameter_updates() {
        let sample_rate = 44_100;
        let control = Arc::new(RealtimeControl::new());
        control.set_target_gain(1.0);
        let mut renderer = RealtimeRenderer::new(
            sample_rate,
            1,
            Arc::clone(&control),
            RealtimeSource::test_tone(sample_rate),
        );
        renderer.sync_controls();
        let mut frame = [0.0];
        for _ in 0..sample_rate / 2 {
            renderer.next_frame(&mut frame);
        }
        let medium_depth = renderer.current_depth();

        control.set_intensity(AudioIntensity::High);
        renderer.sync_controls();
        renderer.next_frame(&mut frame);
        let first = frame[0];
        renderer.next_frame(&mut frame);
        let second = frame[0];
        for _ in 0..sample_rate {
            renderer.next_frame(&mut frame);
        }

        assert!(
            (second - first).abs() < 0.03,
            "click-like step after update"
        );
        assert!(renderer.current_depth() > medium_depth);
        assert!(renderer.current_depth() <= IntensityProfile::high().depth + 1e-4);
    }

    #[test]
    fn all_intensity_profiles_remain_distinct_with_compensation() {
        let profiles = [
            AudioIntensity::Off.profile(),
            AudioIntensity::Low.profile(),
            AudioIntensity::Medium.profile(),
            AudioIntensity::High.profile(),
        ];
        assert_eq!(profiles[0].depth, 0.0);
        assert!(profiles[0].depth < profiles[1].depth);
        assert!(profiles[1].depth < profiles[2].depth);
        assert!(profiles[2].depth < profiles[3].depth);
        assert!(profiles
            .iter()
            .all(|profile| profile.output_compensation > 0.0));
    }

    #[test]
    fn master_volume_has_exact_silence_unity_endpoint_and_smooth_live_changes() {
        let control = Arc::new(RealtimeControl::new());
        control.set_target_gain(1.0);
        control.set_master_volume(100);
        let mut renderer = RealtimeRenderer::new(
            1_000,
            1,
            Arc::clone(&control),
            RealtimeSource::test_tone(1_000),
        );
        renderer.sync_controls();
        let mut frame = [0.0];
        for _ in 0..100 {
            renderer.next_frame(&mut frame);
        }
        let unity = frame[0];
        assert_ne!(unity, 0.0);
        control.set_master_volume(0);
        renderer.sync_controls();
        renderer.next_frame(&mut frame);
        assert!(renderer.master_gain > 0.0 && renderer.master_gain < 1.0);
        for _ in 0..250 {
            renderer.next_frame(&mut frame);
        }
        assert_eq!(renderer.master_gain, 0.0);
        assert_eq!(frame, [0.0]);
        control.set_master_volume(100);
        renderer.sync_controls();
        renderer.next_frame(&mut frame);
        assert!(renderer.master_gain > 0.0 && renderer.master_gain < 1.0);
    }

    fn source_label(item: &str) -> SourceLabel {
        SourceLabel {
            pack_id: "test-pack".to_owned(),
            pack_title: "Test Pack".to_owned(),
            item_id: item.to_owned(),
            item_title: item.to_owned(),
            variant_id: "base".to_owned(),
        }
    }

    #[test]
    fn controller_reports_the_atomic_current_track_label() {
        let tracks = ["first", "second"]
            .into_iter()
            .map(|item| DecodedTrack {
                sample_rate_hz: 1_000,
                channels: 1,
                samples: vec![0.25; 200].into(),
                regions: vec![AuthoredRegion {
                    kind: AuthoredRegionKind::Loop,
                    start_seconds: 0.02,
                    end_seconds: 0.18,
                }],
                label: source_label(item),
            })
            .collect();
        let source = PlaybackSource::Installed(DecodedProgram::new(tracks).unwrap());
        let mut audio = AudioController::new(FakeOutput::default());
        audio
            .start_with_source(source, AudioIntensity::Medium)
            .unwrap();
        assert_eq!(audio.source_label().item_id, "first");
        audio.control.set_current_track(1);
        assert_eq!(audio.source_label().item_id, "second");
    }

    fn navigation_program(track_count: usize) -> DecodedProgram {
        let tracks = (0..track_count)
            .map(|index| DecodedTrack {
                sample_rate_hz: 1_000,
                channels: 2,
                samples: vec![0.25; 80].into(),
                regions: vec![
                    AuthoredRegion {
                        kind: AuthoredRegionKind::Crossfade,
                        start_seconds: 0.004,
                        end_seconds: 0.010,
                    },
                    AuthoredRegion {
                        kind: AuthoredRegionKind::Crossfade,
                        start_seconds: 0.024,
                        end_seconds: 0.034,
                    },
                ],
                label: source_label(&format!("track-{index}")),
            })
            .collect();
        DecodedProgram::new(tracks).unwrap()
    }

    #[test]
    fn controller_navigation_is_directional_commit_driven_and_cleared_by_stop() {
        let mut audio = AudioController::new(FakeOutput::default());
        audio
            .start_with_source(
                PlaybackSource::Installed(navigation_program(2)),
                AudioIntensity::Medium,
            )
            .unwrap();
        assert!(audio.navigation_available());
        assert_eq!(audio.source_label().item_id, "track-0");

        audio.navigate_next().unwrap();
        assert_eq!(audio.control.navigation_target.load(Ordering::Acquire), 1);
        assert_eq!(audio.source_label().item_id, "track-0");
        assert!(!audio.navigation_available());
        assert_eq!(
            audio.navigate_previous(),
            Err(AudioError::NavigationUnavailable)
        );

        audio.stop().unwrap();
        assert_eq!(
            audio.control.navigation_target.load(Ordering::Acquire),
            u8::MAX
        );
        assert!(!audio.control.navigation_active.load(Ordering::Acquire));
    }

    #[test]
    fn controller_navigation_rejects_single_review_paused_and_uses_modulo_direction() {
        let mut single = AudioController::new(FakeOutput::default());
        single
            .start_with_source(
                PlaybackSource::Installed(navigation_program(1)),
                AudioIntensity::Medium,
            )
            .unwrap();
        assert_eq!(
            single.navigate_next(),
            Err(AudioError::NavigationUnavailable)
        );

        let mut review = AudioController::new(FakeOutput::default());
        review
            .start_with_source(
                PlaybackSource::Review(navigation_program(2)),
                AudioIntensity::Medium,
            )
            .unwrap();
        assert_eq!(
            review.navigate_next(),
            Err(AudioError::NavigationUnavailable)
        );

        let mut audio = AudioController::new(FakeOutput::default());
        audio
            .start_with_source(
                PlaybackSource::Installed(navigation_program(2)),
                AudioIntensity::Medium,
            )
            .unwrap();
        audio.pause().unwrap();
        assert_eq!(
            audio.navigate_next(),
            Err(AudioError::NavigationUnavailable)
        );
        audio.resume().unwrap();
        audio.control.set_current_track(1);
        audio.navigate_next().unwrap();
        assert_eq!(audio.control.navigation_target.load(Ordering::Acquire), 0);
        audio.control.clear_navigation();
        audio.control.set_current_track(0);
        audio.navigate_previous().unwrap();
        assert_eq!(audio.control.navigation_target.load(Ordering::Acquire), 1);
    }

    #[test]
    fn installed_test_tone_and_review_keep_distinct_source_kinds() {
        let program = || {
            DecodedProgram::new(vec![DecodedTrack {
                sample_rate_hz: 1_000,
                channels: 1,
                samples: vec![0.25; 200].into(),
                regions: vec![],
                label: source_label("candidate"),
            }])
            .unwrap()
        };
        let mut audio = AudioController::new(FakeOutput::default());
        assert_eq!(audio.source_kind(), PlaybackSourceKind::TestTone);
        audio
            .start_with_source(PlaybackSource::Review(program()), AudioIntensity::Medium)
            .unwrap();
        assert_eq!(audio.source_kind(), PlaybackSourceKind::Review);
        audio.stop().unwrap();
        audio
            .start_with_source(PlaybackSource::Installed(program()), AudioIntensity::Medium)
            .unwrap();
        assert_eq!(audio.source_kind(), PlaybackSourceKind::Installed);
    }

    #[test]
    fn source_freezes_after_fade_out_and_resumes_from_the_same_cursor() {
        let device_program = DeviceProgram {
            tracks: vec![DeviceTrack {
                samples: vec![0.25; 200].into(),
                channels: 1,
                frames: 200,
                regions: vec![AuthoredRegion {
                    kind: AuthoredRegionKind::Loop,
                    start_seconds: 0.02,
                    end_seconds: 0.18,
                }],
                label: source_label("loop"),
            }],
            sample_rate_hz: 1_000,
            channels: 1,
        };
        let control = Arc::new(RealtimeControl::new());
        control.set_target_gain(1.0);
        let source = RealtimeSource::program(device_program).unwrap();
        let mut renderer = RealtimeRenderer::new(1_000, 1, Arc::clone(&control), source);
        renderer.sync_controls();
        let mut frame = [0.0];
        for _ in 0..400 {
            renderer.next_frame(&mut frame);
        }

        control.set_target_gain(0.0);
        renderer.sync_controls();
        for _ in 0..1_000 {
            renderer.next_frame(&mut frame);
            if renderer.gain == 0.0 {
                break;
            }
        }
        assert_eq!(renderer.gain, 0.0);
        let frozen = renderer.source.position();
        for _ in 0..10_000 {
            renderer.next_frame(&mut frame);
            assert_eq!(frame, [0.0]);
        }
        assert_eq!(renderer.source.position(), frozen);

        control.set_target_gain(1.0);
        renderer.sync_controls();
        renderer.next_frame(&mut frame);
        assert_ne!(renderer.source.position(), frozen);
    }

    #[test]
    fn renderer_publishes_track_switch_without_callback_allocation_or_locking() {
        let make_track = |item: &str, value: f32| DeviceTrack {
            samples: vec![value; 40].into(),
            channels: 1,
            frames: 40,
            regions: vec![
                AuthoredRegion {
                    kind: AuthoredRegionKind::Crossfade,
                    start_seconds: 0.004,
                    end_seconds: 0.010,
                },
                AuthoredRegion {
                    kind: AuthoredRegionKind::Crossfade,
                    start_seconds: 0.024,
                    end_seconds: 0.034,
                },
            ],
            label: source_label(item),
        };
        let source = RealtimeSource::program(DeviceProgram {
            tracks: vec![make_track("first", 0.2), make_track("second", 0.3)],
            sample_rate_hz: 1_000,
            channels: 1,
        })
        .unwrap();
        let control = Arc::new(RealtimeControl::new());
        control.set_target_gain(1.0);
        let mut renderer = RealtimeRenderer::new(1_000, 1, Arc::clone(&control), source);
        renderer.sync_controls();
        let mut frame = [0.0];
        for _ in 0..31 {
            renderer.next_frame(&mut frame);
        }
        assert_eq!(control.current_track(), 1);
    }
}
