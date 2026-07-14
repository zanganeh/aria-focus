//! Prebuilt playback sources consumed by the realtime renderer.

use std::f32::consts::FRAC_PI_2;

use crate::media::{
    AuthoredRegionKind, DecodedProgram, DeviceProgram, DeviceTrack, MediaError, SourceLabel,
    MAX_PROGRAM_TRACKS,
};
use crate::playback::RealtimeControl;
use crate::tone::ToneSource;

pub const MAX_TRANSITION_SECONDS: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum PlaybackSource {
    /// Explicit deterministic fallback used when no installed eligible content
    /// exists and by device-independent audio tests.
    TestTone,
    Installed(DecodedProgram),
    /// A hash-pinned, local-only candidate. It intentionally has different
    /// provenance and a review-only provisional transition policy.
    Review(DecodedProgram),
    /// App-owned Music Studio output. This is deliberately separate from
    /// installed and quarantined-review material.
    Draft(DecodedProgram),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackSourceKind {
    TestTone,
    Installed,
    Review,
    Draft,
}

impl PlaybackSource {
    pub fn label(&self) -> SourceLabel {
        self.labels()[0].clone()
    }

    pub fn labels(&self) -> Vec<SourceLabel> {
        match self {
            Self::TestTone => vec![SourceLabel::test_fallback()],
            Self::Installed(program) | Self::Review(program) | Self::Draft(program) => program
                .tracks
                .iter()
                .map(|track| track.label.clone())
                .collect(),
        }
    }

    pub fn kind(&self) -> PlaybackSourceKind {
        match self {
            Self::TestTone => PlaybackSourceKind::TestTone,
            Self::Installed(_) => PlaybackSourceKind::Installed,
            Self::Review(_) => PlaybackSourceKind::Review,
            Self::Draft(_) => PlaybackSourceKind::Draft,
        }
    }
}

pub(crate) enum RealtimeSource {
    Tone { tone: ToneSource, current: f32 },
    Program(ProgramRenderer),
}

impl RealtimeSource {
    pub(crate) fn test_tone(sample_rate: u32) -> Self {
        Self::Tone {
            tone: ToneSource::new(sample_rate),
            current: 0.0,
        }
    }

    pub(crate) fn program(program: DeviceProgram) -> Result<Self, MediaError> {
        Ok(Self::Program(ProgramRenderer::new(program)?))
    }

    /// Review candidates have no asserted authored loop evidence.  This is an
    /// explicit, review-only crossfade at the file boundary so a 45–90 minute
    /// session continues while the reviewer evaluates that transition.
    pub(crate) fn review_program(program: DeviceProgram) -> Result<Self, MediaError> {
        Ok(Self::Program(ProgramRenderer::new_provisional_review(
            program,
        )?))
    }

    /// Drafts have no authored loop assertions yet, so use the same bounded
    /// provisional single-track boundary treatment as review material.
    pub(crate) fn draft_program(program: DeviceProgram) -> Result<Self, MediaError> {
        Ok(Self::Program(ProgramRenderer::new_provisional_review(
            program,
        )?))
    }

    pub(crate) fn begin_frame(&mut self) {
        if let Self::Tone { tone, current } = self {
            *current = tone.next_sample();
        }
    }

    #[inline]
    pub(crate) fn sample(&self, channel: usize) -> f32 {
        match self {
            Self::Tone { current, .. } => *current,
            Self::Program(program) => program.sample(channel),
        }
    }

    pub(crate) fn end_frame(&mut self) -> usize {
        match self {
            Self::Tone { .. } => 0,
            Self::Program(program) => {
                program.advance();
                program.current_track()
            }
        }
    }

    pub(crate) fn request_navigation(&mut self, target: usize, control: &RealtimeControl) {
        if let Self::Program(program) = self {
            program.request_navigation(target, control);
        }
    }

    pub(crate) fn navigation_active(&self) -> bool {
        matches!(self, Self::Program(program) if program.navigation_active())
    }

    #[cfg(test)]
    pub(crate) fn position(&self) -> (usize, usize) {
        match self {
            Self::Tone { .. } => (0, 0),
            Self::Program(program) => program.position(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameRegion {
    start: usize,
    end: usize,
}

impl FrameRegion {
    fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transition {
    Crossfade {
        outgoing_start: usize,
        incoming_start: usize,
        length: usize,
        next_track: usize,
    },
    Loop {
        incoming_start: usize,
        end: usize,
        outgoing_start: usize,
        length: usize,
    },
}

/// Bounded state machine over one or two immutable, pre-resampled tracks.
pub struct ProgramRenderer {
    program: DeviceProgram,
    transitions: Vec<Transition>,
    track: usize,
    frame: usize,
    program_gain: f32,
    manual: Option<ManualTransition>,
    automatic_navigation_target: Option<usize>,
}

/// A callback-local manual transition. It is constructed from already validated
/// authored crossfade regions; no media or control-path data is touched while rendering.
#[derive(Debug, Clone, Copy)]
struct ManualTransition {
    outgoing_track: usize,
    outgoing_frame: usize,
    incoming_track: usize,
    incoming_start: usize,
    offset: usize,
    length: usize,
}

impl ProgramRenderer {
    pub fn new(program: DeviceProgram) -> Result<Self, MediaError> {
        let track_count = program.tracks.len();
        if track_count == 0 || track_count > MAX_PROGRAM_TRACKS {
            return Err(MediaError::InvalidProgramSize(track_count));
        }
        let transitions = match track_count {
            1 => vec![loop_transition(&program.tracks[0], program.sample_rate_hz)?],
            2 => match crossfade_cycle(
                &program.tracks[0],
                &program.tracks[1],
                program.sample_rate_hz,
            ) {
                Ok([first_to_second, second_to_first]) => {
                    vec![first_to_second, second_to_first]
                }
                Err(_) => loop_queue_transitions(&program)?,
            },
            _ => loop_queue_transitions(&program)?,
        };
        let program_gain = constant_program_gain(&program, &transitions)?;
        Ok(Self {
            program,
            transitions,
            track: 0,
            frame: 0,
            program_gain,
            manual: None,
            automatic_navigation_target: None,
        })
    }

    /// Creates the deliberately provisional review loop. This does not use or
    /// validate authored loop metadata and must never be used for packs.
    pub fn new_provisional_review(program: DeviceProgram) -> Result<Self, MediaError> {
        if program.tracks.len() != 1 {
            return Err(MediaError::InvalidProgramSize(program.tracks.len()));
        }
        let transitions = vec![provisional_review_loop_transition(
            &program.tracks[0],
            program.sample_rate_hz,
        )?];
        let program_gain = constant_program_gain(&program, &transitions)?;
        Ok(Self {
            program,
            transitions,
            track: 0,
            frame: 0,
            program_gain,
            manual: None,
            automatic_navigation_target: None,
        })
    }

    #[inline]
    pub fn sample(&self, channel: usize) -> f32 {
        if let Some(manual) = self.manual {
            return blend(
                track_sample(
                    &self.program.tracks[manual.outgoing_track],
                    manual.outgoing_frame + manual.offset,
                    channel,
                ),
                track_sample(
                    &self.program.tracks[manual.incoming_track],
                    manual.incoming_start + manual.offset,
                    channel,
                ),
                manual.offset,
                manual.length,
            ) * self.program_gain;
        }
        let raw = match self.transitions[self.track] {
            Transition::Crossfade {
                outgoing_start,
                incoming_start,
                length,
                next_track,
            } if self.frame >= outgoing_start && self.frame < outgoing_start + length => {
                let offset = self.frame - outgoing_start;
                blend(
                    track_sample(&self.program.tracks[self.track], self.frame, channel),
                    track_sample(
                        &self.program.tracks[next_track],
                        incoming_start + offset,
                        channel,
                    ),
                    offset,
                    length,
                )
            }
            Transition::Loop {
                incoming_start,
                outgoing_start,
                length,
                ..
            } if self.frame >= outgoing_start && self.frame < outgoing_start + length => {
                let offset = self.frame - outgoing_start;
                let track = &self.program.tracks[self.track];
                blend(
                    track_sample(track, self.frame, channel),
                    track_sample(track, incoming_start + offset, channel),
                    offset,
                    length,
                )
            }
            _ => track_sample(&self.program.tracks[self.track], self.frame, channel),
        };
        raw * self.program_gain
    }

    #[inline]
    pub fn advance(&mut self) {
        if let Some(mut manual) = self.manual {
            manual.offset += 1;
            if manual.offset >= manual.length {
                self.track = manual.incoming_track;
                self.frame = manual.incoming_start + manual.length;
                self.manual = None;
            } else {
                self.manual = Some(manual);
            }
            return;
        }
        match self.transitions[self.track] {
            Transition::Crossfade {
                outgoing_start,
                incoming_start,
                length,
                next_track,
            } if self.frame + 1 >= outgoing_start + length => {
                self.track = next_track;
                self.frame = incoming_start + length;
                if self.automatic_navigation_target == Some(next_track) {
                    self.automatic_navigation_target = None;
                }
            }
            Transition::Loop {
                incoming_start,
                end,
                length,
                ..
            } if self.frame + 1 >= end => {
                self.frame = incoming_start + length;
            }
            _ => self.frame += 1,
        }
    }

    pub fn current_track(&self) -> usize {
        self.track
    }

    pub(crate) fn navigation_active(&self) -> bool {
        self.manual.is_some() || self.automatic_navigation_target.is_some()
    }

    /// Latest requests during a manual transition are rejected by the controller
    /// while `navigation_active` is set. This keeps the transition cursor fixed.
    pub(crate) fn request_navigation(&mut self, target: usize, control: &RealtimeControl) {
        if self.navigation_active() || target >= self.program.tracks.len() || target == self.track {
            control.set_navigation_active(false);
            return;
        }
        match self.transitions[self.track] {
            Transition::Crossfade {
                outgoing_start,
                incoming_start,
                length,
                next_track,
            } => {
                // If the authored transition to the requested track is already audible, let it
                // finish unchanged. The control remains pending until the actual track commit.
                if self.frame >= outgoing_start && target == next_track {
                    self.automatic_navigation_target = Some(target);
                    control.set_navigation_active(true);
                    return;
                }
                // Before the authored transition begins, both fixed spans must fit.
                let outgoing = &self.program.tracks[self.track];
                if self.frame.saturating_add(length) > outgoing.frames
                    || incoming_start.saturating_add(length) > self.program.tracks[target].frames
                {
                    control.set_navigation_active(false);
                    return;
                }
                self.manual = Some(ManualTransition {
                    outgoing_track: self.track,
                    outgoing_frame: self.frame,
                    incoming_track: target,
                    incoming_start,
                    offset: 0,
                    length,
                });
                control.set_navigation_active(true);
            }
            Transition::Loop { length, .. } => {
                // Multi-track loop queue: crossfade from the current playback
                // position into the target track's authored loop incoming
                // region. The constant program-gain headroom (sqrt(2) times the
                // largest individual sample for any program with more than one
                // track) covers any such equal-power pairing, so the manual
                // transition stays bounded without per-pair validation.
                let Transition::Loop {
                    incoming_start: target_incoming,
                    length: target_length,
                    ..
                } = self.transitions[target]
                else {
                    control.set_navigation_active(false);
                    return;
                };
                let outgoing = &self.program.tracks[self.track];
                let incoming = &self.program.tracks[target];
                let length = length
                    .min(target_length)
                    .min(outgoing.frames.saturating_sub(self.frame))
                    .min(incoming.frames.saturating_sub(target_incoming));
                if length < 2 {
                    control.set_navigation_active(false);
                    return;
                }
                self.manual = Some(ManualTransition {
                    outgoing_track: self.track,
                    outgoing_frame: self.frame,
                    incoming_track: target,
                    incoming_start: target_incoming,
                    offset: 0,
                    length,
                });
                control.set_navigation_active(true);
            }
        }
    }

    #[cfg(test)]
    fn position(&self) -> (usize, usize) {
        (self.track, self.frame)
    }

    #[cfg(test)]
    fn program_gain(&self) -> f32 {
        self.program_gain
    }
}

fn provisional_review_loop_transition(
    track: &DeviceTrack,
    sample_rate: u32,
) -> Result<Transition, MediaError> {
    // A short boundary crossfade is a playback aid, not evidence of a safe or
    // seamless authored loop. Keep enough non-overlapping source material.
    let length = seconds_to_frame(2.0, sample_rate).min(track.frames / 3);
    if length < 2 {
        return Err(MediaError::MissingContinuousTransition);
    }
    Ok(Transition::Loop {
        incoming_start: 0,
        end: track.frames,
        outgoing_start: track.frames - length,
        length,
    })
}

/// Build a self-loop transition for every track in a multi-track loop queue.
///
/// The catalogue bounds a loop queue at up to `MAX_PROGRAM_TRACKS` distinct
/// loop-safe tracks; each track loops within its own authored Loop region and
/// manual previous/next navigation crossfades between tracks. This preserves
/// the two-track authored crossfade cycle for crossfade pairs while letting a
/// bounded loop queue of more than two tracks play and navigate continuously.
fn loop_queue_transitions(program: &DeviceProgram) -> Result<Vec<Transition>, MediaError> {
    program
        .tracks
        .iter()
        .map(|track| loop_transition(track, program.sample_rate_hz))
        .collect()
}

fn crossfade_cycle(
    first: &DeviceTrack,
    second: &DeviceTrack,
    sample_rate: u32,
) -> Result<[Transition; 2], MediaError> {
    let first_regions = frame_regions(first, AuthoredRegionKind::Crossfade, sample_rate)?;
    let second_regions = frame_regions(second, AuthoredRegionKind::Crossfade, sample_rate)?;
    let maximum = seconds_to_frame(MAX_TRANSITION_SECONDS, sample_rate);
    for first_incoming in &first_regions {
        for first_outgoing in &first_regions {
            for second_incoming in &second_regions {
                for second_outgoing in &second_regions {
                    let first_to_second_length =
                        first_outgoing.len().min(second_incoming.len()).min(maximum);
                    let second_to_first_length =
                        second_outgoing.len().min(first_incoming.len()).min(maximum);
                    if first_to_second_length < 2
                        || second_to_first_length < 2
                        || second_incoming.start + first_to_second_length > second_outgoing.start
                        || first_incoming.start + second_to_first_length > first_outgoing.start
                    {
                        continue;
                    }
                    return Ok([
                        Transition::Crossfade {
                            outgoing_start: first_outgoing.start,
                            incoming_start: second_incoming.start,
                            length: first_to_second_length,
                            next_track: 1,
                        },
                        Transition::Crossfade {
                            outgoing_start: second_outgoing.start,
                            incoming_start: first_incoming.start,
                            length: second_to_first_length,
                            next_track: 0,
                        },
                    ]);
                }
            }
        }
    }
    Err(MediaError::MissingContinuousTransition)
}

fn loop_transition(track: &DeviceTrack, sample_rate: u32) -> Result<Transition, MediaError> {
    let region = frame_regions(track, AuthoredRegionKind::Loop, sample_rate)?
        .into_iter()
        .next()
        .ok_or(MediaError::MissingContinuousTransition)?;
    let length = region
        .len()
        .saturating_div(2)
        .min(seconds_to_frame(MAX_TRANSITION_SECONDS, sample_rate));
    if length < 2 {
        return Err(MediaError::MissingContinuousTransition);
    }
    Ok(Transition::Loop {
        incoming_start: region.start,
        end: region.end,
        outgoing_start: region.end - length,
        length,
    })
}

fn frame_regions(
    track: &DeviceTrack,
    kind: AuthoredRegionKind,
    sample_rate: u32,
) -> Result<Vec<FrameRegion>, MediaError> {
    let mut regions = Vec::new();
    for region in track.regions.iter().filter(|region| region.kind == kind) {
        if !region.start_seconds.is_finite()
            || !region.end_seconds.is_finite()
            || region.start_seconds < 0.0
            || region.end_seconds <= region.start_seconds
        {
            return Err(MediaError::MissingContinuousTransition);
        }
        let start = seconds_to_frame(region.start_seconds, sample_rate);
        let end = seconds_to_frame(region.end_seconds, sample_rate);
        if start >= end || end > track.frames {
            return Err(MediaError::MissingContinuousTransition);
        }
        regions.push(FrameRegion { start, end });
    }
    regions.sort_by_key(|region| (region.start, region.end));
    Ok(regions)
}

fn constant_program_gain(
    program: &DeviceProgram,
    transitions: &[Transition],
) -> Result<f32, MediaError> {
    let mut peak = 0.0f32;
    let mut individual_peak = 0.0f32;
    for track in &program.tracks {
        for sample in track.samples.iter().copied() {
            if !sample.is_finite() {
                return Err(MediaError::NonFiniteSample);
            }
            peak = peak.max(sample.abs());
            individual_peak = individual_peak.max(sample.abs());
        }
    }
    for (track_index, transition) in transitions.iter().copied().enumerate() {
        match transition {
            Transition::Crossfade {
                outgoing_start,
                incoming_start,
                length,
                next_track,
            } => {
                for offset in 0..length {
                    for channel in 0..program.channels {
                        let mixed = blend(
                            track_sample(
                                &program.tracks[track_index],
                                outgoing_start + offset,
                                channel,
                            ),
                            track_sample(
                                &program.tracks[next_track],
                                incoming_start + offset,
                                channel,
                            ),
                            offset,
                            length,
                        );
                        if !mixed.is_finite() {
                            return Err(MediaError::NonFiniteSample);
                        }
                        peak = peak.max(mixed.abs());
                    }
                }
            }
            Transition::Loop {
                incoming_start,
                outgoing_start,
                length,
                ..
            } => {
                for offset in 0..length {
                    for channel in 0..program.channels {
                        let mixed = blend(
                            track_sample(
                                &program.tracks[track_index],
                                outgoing_start + offset,
                                channel,
                            ),
                            track_sample(
                                &program.tracks[track_index],
                                incoming_start + offset,
                                channel,
                            ),
                            offset,
                            length,
                        );
                        if !mixed.is_finite() {
                            return Err(MediaError::NonFiniteSample);
                        }
                        peak = peak.max(mixed.abs());
                    }
                }
            }
        }
    }
    // A manual equal-power transition can pair any current outgoing frame with
    // a validated incoming region. Its worst-case correlated peak is sqrt(2)
    // times the largest individual sample, even when authored auto-regions do
    // not contain that particular pairing.
    if program.tracks.len() > 1 {
        peak = peak.max(individual_peak * std::f32::consts::SQRT_2);
    }
    Ok(if peak > 1.0 { peak.recip() } else { 1.0 })
}

#[inline]
fn track_sample(track: &DeviceTrack, frame: usize, channel: usize) -> f32 {
    track.samples[frame * track.channels + channel]
}

#[inline]
fn blend(outgoing: f32, incoming: f32, offset: usize, length: usize) -> f32 {
    let (outgoing_gain, incoming_gain) = blend_coefficients(offset, length);
    outgoing * outgoing_gain + incoming * incoming_gain
}

#[inline]
fn blend_coefficients(offset: usize, length: usize) -> (f32, f32) {
    if offset == 0 {
        return (1.0, 0.0);
    }
    if offset + 1 == length {
        return (0.0, 1.0);
    }
    let phase = offset as f32 / (length - 1) as f32 * FRAC_PI_2;
    (phase.cos(), phase.sin())
}

fn seconds_to_frame(seconds: f32, sample_rate: u32) -> usize {
    (seconds * sample_rate as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AuthoredRegion, SourceLabel};

    fn label(item: &str) -> SourceLabel {
        SourceLabel {
            pack_id: "test-pack".to_owned(),
            pack_title: "Test Pack".to_owned(),
            item_id: item.to_owned(),
            item_title: item.to_owned(),
            variant_id: "source".to_owned(),
        }
    }

    fn crossfade_track(item: &str, left: f32, right: f32) -> DeviceTrack {
        let mut samples = Vec::new();
        for _ in 0..40 {
            samples.extend_from_slice(&[left, right]);
        }
        DeviceTrack {
            samples: samples.into(),
            channels: 2,
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
            label: label(item),
        }
    }

    fn crossfade_program(first: (f32, f32), second: (f32, f32)) -> DeviceProgram {
        DeviceProgram {
            tracks: vec![
                crossfade_track("first", first.0, first.1),
                crossfade_track("second", second.0, second.1),
            ],
            sample_rate_hz: 1_000,
            channels: 2,
        }
    }

    #[test]
    fn paired_authored_regions_align_endpoints_and_cursor_continuation() {
        let mut first = crossfade_track("first", 0.25, -0.25);
        let mut second = crossfade_track("second", 0.75, -0.75);
        for frame in 0..40 {
            first.samples = {
                let mut data = first.samples.to_vec();
                data[frame * 2] = frame as f32 / 100.0;
                data[(frame * 2) + 1] = -(frame as f32 / 100.0);
                data.into()
            };
            second.samples = {
                let mut data = second.samples.to_vec();
                data[frame * 2] = 0.5 + frame as f32 / 100.0;
                data[(frame * 2) + 1] = -(0.5 + frame as f32 / 100.0);
                data.into()
            };
        }
        let mut renderer = ProgramRenderer::new(DeviceProgram {
            tracks: vec![first, second],
            sample_rate_hz: 1_000,
            channels: 2,
        })
        .unwrap();
        while renderer.position() != (0, 24) {
            renderer.advance();
        }
        assert_eq!(renderer.sample(0), 0.24 * renderer.program_gain());
        for _ in 0..5 {
            renderer.advance();
        }
        assert_eq!(renderer.position(), (0, 29));
        assert_eq!(
            renderer.sample(0),
            (0.5 + 9.0 / 100.0) * renderer.program_gain()
        );
        renderer.advance();
        assert_eq!(renderer.position(), (1, 10));
        assert_eq!(renderer.sample(0), 0.60 * renderer.program_gain());
    }

    #[test]
    fn constant_headroom_bounds_full_scale_correlated_and_opposite_polarity_material() {
        for second in [(1.0, -1.0), (-1.0, 1.0)] {
            let mut renderer =
                ProgramRenderer::new(crossfade_program((1.0, -1.0), second)).unwrap();
            let gain = renderer.program_gain();
            assert!(gain > 0.0 && gain <= 1.0);
            let mut peak = 0.0f32;
            let mut floor = f32::MAX;
            for _ in 0..200 {
                for channel in 0..2 {
                    let sample = renderer.sample(channel);
                    assert!(sample.is_finite());
                    peak = peak.max(sample.abs());
                    floor = floor.min(sample.abs());
                }
                renderer.advance();
            }
            assert!(peak <= 1.0 + 1e-6);
            assert!(floor > 0.0);
        }
    }

    #[test]
    fn stereo_channels_never_swap_and_alternation_repeats_forever() {
        let mut renderer =
            ProgramRenderer::new(crossfade_program((0.8, -0.6), (0.4, -0.2))).unwrap();
        let mut switches = 0;
        let mut previous_track = 0;
        for _ in 0..500 {
            assert!(renderer.sample(0) >= 0.0);
            assert!(renderer.sample(1) <= 0.0);
            renderer.advance();
            if renderer.current_track() != previous_track {
                switches += 1;
                previous_track = renderer.current_track();
            }
        }
        assert!(switches >= 10);
    }

    #[test]
    fn manual_navigation_crossfades_without_early_identity_commit_or_channel_swap() {
        let control = RealtimeControl::new();
        let mut renderer =
            ProgramRenderer::new(crossfade_program((0.8, -0.6), (0.4, -0.2))).unwrap();
        let start_left = renderer.sample(0);
        renderer.request_navigation(1, &control);
        assert!(renderer.navigation_active());
        assert_eq!(renderer.current_track(), 0);
        assert_eq!(renderer.sample(0), start_left);

        let mut rendered = 0;
        while renderer.navigation_active() {
            let left = renderer.sample(0);
            let right = renderer.sample(1);
            assert!(left.is_finite() && right.is_finite());
            assert!((-1.0..=1.0).contains(&left));
            assert!((-1.0..=1.0).contains(&right));
            assert!(left >= 0.0 && right <= 0.0, "stereo channels must not swap");
            renderer.advance();
            rendered += 1;
            if renderer.navigation_active() {
                assert_eq!(renderer.current_track(), 0);
            }
            assert!(rendered <= 10);
        }
        assert_eq!(renderer.current_track(), 1);
        assert_eq!(rendered, 6);
    }

    #[test]
    fn navigation_during_authored_crossfade_waits_for_the_real_commit() {
        let control = RealtimeControl::new();
        let mut renderer =
            ProgramRenderer::new(crossfade_program((0.8, -0.6), (0.4, -0.2))).unwrap();
        while renderer.position() != (0, 29) {
            renderer.advance();
        }
        renderer.request_navigation(1, &control);
        assert!(renderer.navigation_active());
        assert_eq!(renderer.current_track(), 0);
        while renderer.navigation_active() {
            renderer.advance();
        }
        assert_eq!(renderer.current_track(), 1);
    }

    #[test]
    fn manual_crossfade_headroom_covers_arbitrary_correlated_frames() {
        let mut program = crossfade_program((1.0, -1.0), (1.0, -1.0));
        // Make the authored automatic outgoing region anti-correlated with the
        // incoming region while leaving the arbitrary manual start correlated.
        for frame in 24..34 {
            program.tracks[0].samples = {
                let mut samples = program.tracks[0].samples.to_vec();
                samples[frame * 2] = -1.0;
                samples[(frame * 2) + 1] = 1.0;
                samples.into()
            };
        }
        let control = RealtimeControl::new();
        let mut renderer = ProgramRenderer::new(program).unwrap();
        assert!(renderer.program_gain() <= std::f32::consts::FRAC_1_SQRT_2 + 1.0e-6);
        renderer.request_navigation(1, &control);
        while renderer.navigation_active() {
            for channel in 0..2 {
                let sample = renderer.sample(channel);
                assert!(sample.is_finite());
                assert!(sample.abs() <= 1.0 + 1.0e-6);
            }
            renderer.advance();
        }
    }

    #[test]
    fn authored_loop_repeats_without_gap_and_preserves_track_identity() {
        let track = DeviceTrack {
            samples: vec![1.0; 30].into(),
            channels: 1,
            frames: 30,
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.004,
                end_seconds: 0.024,
            }],
            label: label("loop"),
        };
        let mut renderer = ProgramRenderer::new(DeviceProgram {
            tracks: vec![track],
            sample_rate_hz: 1_000,
            channels: 1,
        })
        .unwrap();
        let mut wraps = 0;
        let mut previous_frame = 0;
        for _ in 0..300 {
            let sample = renderer.sample(0);
            assert!(sample.is_finite() && sample > 0.0 && sample <= 1.0);
            renderer.advance();
            let (_, frame) = renderer.position();
            if frame < previous_frame {
                wraps += 1;
            }
            previous_frame = frame;
            assert_eq!(renderer.current_track(), 0);
        }
        assert!(wraps >= 10);
    }

    #[test]
    fn provisional_review_loop_repeats_without_claiming_authored_regions() {
        let track = DeviceTrack {
            samples: (0..300)
                .map(|frame| frame as f32 / 300.0)
                .collect::<Vec<_>>()
                .into(),
            channels: 1,
            frames: 300,
            regions: vec![],
            label: label("review"),
        };
        let mut renderer = ProgramRenderer::new_provisional_review(DeviceProgram {
            tracks: vec![track],
            sample_rate_hz: 100,
            channels: 1,
        })
        .unwrap();
        let mut wraps = 0;
        let mut prior = 0;
        for _ in 0..1_000 {
            assert!(renderer.sample(0).is_finite());
            renderer.advance();
            let (_, frame) = renderer.position();
            if frame < prior {
                wraps += 1;
            }
            prior = frame;
        }
        assert!(wraps >= 3);
    }

    #[test]
    fn invalid_or_clamped_regions_are_rejected() {
        let mut program = crossfade_program((0.5, -0.5), (0.5, -0.5));
        program.tracks[0].regions[1].end_seconds = 0.050;
        assert!(matches!(
            ProgramRenderer::new(program),
            Err(MediaError::MissingContinuousTransition)
        ));
    }

    fn loop_queue_track(item: &str, value: f32) -> DeviceTrack {
        DeviceTrack {
            samples: vec![value; 200].into(),
            channels: 1,
            frames: 200,
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.02,
                end_seconds: 0.18,
            }],
            label: label(item),
        }
    }

    fn loop_queue_program(values: &[f32]) -> DeviceProgram {
        DeviceProgram {
            tracks: values
                .iter()
                .enumerate()
                .map(|(index, value)| loop_queue_track(&format!("loop-{index}"), *value))
                .collect(),
            sample_rate_hz: 1_000,
            channels: 1,
        }
    }

    fn commit_navigation(
        renderer: &mut ProgramRenderer,
        control: &RealtimeControl,
        target: usize,
        expected_value: f32,
        gain: f32,
    ) {
        renderer.request_navigation(target, control);
        assert!(
            renderer.navigation_active(),
            "navigation to track {target} should be accepted"
        );
        while renderer.navigation_active() {
            let sample = renderer.sample(0);
            assert!(sample.is_finite());
            assert!(sample.abs() <= 1.0 + 1e-6);
            renderer.advance();
        }
        assert_eq!(
            renderer.current_track(),
            target,
            "renderer must commit to the requested track"
        );
        let sample = renderer.sample(0);
        assert!(
            (sample - expected_value * gain).abs() < 1e-6,
            "committed track {target} must expose its own identity sample"
        );
    }

    #[test]
    fn loop_queue_navigates_across_four_tracks_with_wrap_direction_and_commit_identity() {
        let values = [0.10_f32, 0.20, 0.30, 0.40];
        let program = loop_queue_program(&values);
        let control = RealtimeControl::new();
        let mut renderer = ProgramRenderer::new(program).unwrap();
        let gain = renderer.program_gain();

        assert_eq!(renderer.current_track(), 0);
        // Forward jump into an arbitrary index beyond the historic two-track cap.
        commit_navigation(&mut renderer, &control, 2, values[2], gain);
        // Backward navigation to the primary.
        commit_navigation(&mut renderer, &control, 0, values[0], gain);
        // Previous-wrap: from track 0 the previous index wraps to the last track.
        commit_navigation(&mut renderer, &control, 3, values[3], gain);
        // Next-wrap: from the last track the next index wraps back to track 0.
        commit_navigation(&mut renderer, &control, 0, values[0], gain);
    }

    #[test]
    fn two_track_authored_crossfade_cycle_is_preserved_over_loop_queue() {
        // A two-track program with paired authored crossfade regions must keep
        // its automatic bidirectional crossfade cycle. Falling back to the
        // bounded loop queue would instead self-loop each track and never
        // switch between them, so the bounded-frame guards below fail closed.
        let mut renderer =
            ProgramRenderer::new(crossfade_program((0.8, -0.6), (0.4, -0.2))).unwrap();
        let mut frames = 0;
        while renderer.current_track() == 0 {
            frames += 1;
            assert!(frames <= 100, "track 0 never committed to track 1");
            assert!(renderer.sample(0) >= 0.0);
            renderer.advance();
        }
        assert_eq!(renderer.current_track(), 1);
        frames = 0;
        while renderer.current_track() == 1 {
            frames += 1;
            assert!(frames <= 100, "track 1 never committed back to track 0");
            assert!(renderer.sample(0) >= 0.0);
            renderer.advance();
        }
        assert_eq!(renderer.current_track(), 0);
    }
}
