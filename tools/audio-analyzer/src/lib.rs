//! Deterministic, offline technical analysis for candidate music assets.
//!
//! This crate never mutates source audio and never assigns human-QA status.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ebur128::{EbuR128, Mode};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use serde::Serialize;
use sha2::{Digest, Sha256};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_S16BE,
    CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32BE,
    CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U16BE, CODEC_ID_PCM_U16LE, CODEC_ID_PCM_U24BE,
    CODEC_ID_PCM_U24LE, CODEC_ID_PCM_U32BE, CODEC_ID_PCM_U32LE,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tempfile::NamedTempFile;

const MAX_ENCODED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DECODED_SAMPLES: usize = 64 * 1024 * 1024;
const SILENCE_AMPLITUDE: f32 = 0.000_1;
const REPORTABLE_SILENCE_SECONDS: f64 = 0.100;
const REJECT_SILENCE_SECONDS: f64 = 1.000;
const CLICK_DELTA: f32 = 0.75;
const SPECTRUM_WINDOW: usize = 2048;
const SPECTRUM_HOP: usize = 1024;
const HIGH_FREQUENCY_HZ: f64 = 8_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Wav,
    Flac,
    Mp3,
}

impl Codec {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
        }
    }

    fn format_name(self) -> &'static str {
        match self {
            Self::Wav => "wave",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
        }
    }

    fn accepts(self, codec: AudioCodecId) -> bool {
        match self {
            Self::Wav => pcm_bit_depth(codec).is_some_and(|bits| matches!(bits, 16 | 24 | 32)),
            Self::Flac => codec == CODEC_ID_FLAC,
            Self::Mp3 => codec == CODEC_ID_MP3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub analyzer: AnalyzerIdentity,
    pub source: SourceIdentity,
    pub decode: DecodeReport,
    pub measurements: Measurements,
    pub unassessed: UnassessedMeasurements,
    pub hard_rejections: Vec<Reason>,
    pub flags: Vec<Reason>,
}

impl AnalysisReport {
    pub fn has_hard_rejections(&self) -> bool {
        !self.hard_rejections.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalyzerIdentity {
    pub name: &'static str,
    pub version: &'static str,
    pub deterministic_contract: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceIdentity {
    pub file_name: String,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecodeReport {
    pub status: DecodeStatus,
    pub codec: Option<Codec>,
    pub sample_rate_hz: Option<u32>,
    pub channels: Option<u16>,
    pub bit_depth: Option<u16>,
    pub frames: Option<u64>,
    pub duration_seconds: Option<f64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeStatus {
    Decoded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Measurements {
    pub integrated_lufs: NumericMetric,
    pub true_peak_dbtp: NumericMetric,
    pub loudness_range_lu: NumericMetric,
    pub short_window_loudness_range_approx_lu: NumericMetric,
    pub spectral_centroid_hz: NumericMetric,
    pub high_frequency_energy_ratio: NumericMetric,
    pub onset_density_per_second: NumericMetric,
    pub silence: SilenceMeasurement,
    pub clipped_samples: u64,
    pub non_finite_samples: u64,
    pub discontinuity_candidates: DiscontinuityMeasurement,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NumericMetric {
    pub status: MetricStatus,
    pub value: Option<f64>,
    pub unit: &'static str,
    pub method: &'static str,
    pub reason: Option<String>,
}

impl NumericMetric {
    fn measured(value: f64, unit: &'static str, method: &'static str) -> Self {
        Self {
            status: MetricStatus::Measured,
            value: Some(round_six(value)),
            unit,
            method,
            reason: None,
        }
    }

    fn unavailable(unit: &'static str, method: &'static str, reason: impl Into<String>) -> Self {
        Self {
            status: MetricStatus::Unavailable,
            value: None,
            unit,
            method,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStatus {
    Measured,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SilenceMeasurement {
    pub threshold_amplitude: f64,
    pub total_near_silence_seconds: f64,
    pub longest_near_silence_seconds: f64,
    pub regions: Vec<TimeRegion>,
    pub method: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimeRegion {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiscontinuityMeasurement {
    pub threshold_inter_sample_delta: f64,
    pub candidate_count: u64,
    pub maximum_inter_sample_delta: f64,
    pub candidate_times_seconds: Vec<f64>,
    pub candidate_times_truncated: bool,
    pub method: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnassessedMeasurements {
    pub tempo_bpm: NotAssessed,
    pub tempo_confidence: NotAssessed,
    pub tempo_drift_percent: NotAssessed,
    pub section_change_novelty: NotAssessed,
    pub vocal_speech_likelihood: NotAssessed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotAssessed {
    pub status: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reason {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
struct DecodedAsset {
    source: SourceIdentity,
    codec: Codec,
    sample_rate_hz: u32,
    channels: u16,
    bit_depth: Option<u16>,
    samples: Vec<f32>,
}

#[derive(Debug)]
struct SourceSnapshot {
    source: SourceIdentity,
    codec: Codec,
    file: NamedTempFile,
}

#[derive(Debug)]
struct DecodeFailure {
    source: SourceIdentity,
    codec: Option<Codec>,
    code: &'static str,
    message: String,
}

/// Analyze one WAV, FLAC, or MP3 candidate without modifying it.
pub fn analyze_file(path: &Path) -> AnalysisReport {
    match decode_file(path) {
        Ok(asset) => analyze_decoded(asset),
        Err(failure) => failure_report(failure),
    }
}

fn analyze_decoded(asset: DecodedAsset) -> AnalysisReport {
    let channels = usize::from(asset.channels);
    let frames = asset.samples.len() / channels;
    let duration = frames as f64 / f64::from(asset.sample_rate_hz);
    let non_finite_samples = asset
        .samples
        .iter()
        .filter(|sample| !sample.is_finite())
        .count() as u64;
    let clipped_samples = asset
        .samples
        .iter()
        .filter(|sample| sample.is_finite() && sample.abs() >= 1.0)
        .count() as u64;

    let (silence, discontinuities) =
        time_domain_measurements(&asset.samples, asset.sample_rate_hz, asset.channels);
    let mut hard_rejections = Vec::new();
    if non_finite_samples > 0 {
        hard_rejections.push(reason(
            "non_finite_samples",
            format!("decoded PCM contains {non_finite_samples} non-finite samples"),
        ));
    }
    if clipped_samples > 0 {
        hard_rejections.push(reason(
            "clipped_samples",
            format!("decoded PCM contains {clipped_samples} samples at or beyond full scale"),
        ));
    }
    if silence.longest_near_silence_seconds >= REJECT_SILENCE_SECONDS {
        hard_rejections.push(reason(
            "unexplained_near_silence",
            format!(
                "near-silence lasts {:.3} seconds; candidate review has not explained it",
                silence.longest_near_silence_seconds
            ),
        ));
    }
    let measurements = if non_finite_samples == 0 {
        measured_metrics(
            &asset.samples,
            asset.sample_rate_hz,
            asset.channels,
            silence,
            clipped_samples,
            discontinuities,
        )
    } else {
        unavailable_metrics(
            "non-finite PCM prevents reliable measurement",
            silence,
            clipped_samples,
            non_finite_samples,
            discontinuities,
        )
    };
    let flags = calibration_flags(&measurements);

    AnalysisReport {
        schema_version: 1,
        analyzer: analyzer_identity(),
        source: asset.source,
        decode: DecodeReport {
            status: DecodeStatus::Decoded,
            codec: Some(asset.codec),
            sample_rate_hz: Some(asset.sample_rate_hz),
            channels: Some(asset.channels),
            bit_depth: asset.bit_depth,
            frames: Some(frames as u64),
            duration_seconds: Some(round_six(duration)),
            error_code: None,
            error_message: None,
        },
        measurements,
        unassessed: unassessed(),
        hard_rejections,
        flags,
    }
}

fn measured_metrics(
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
    silence: SilenceMeasurement,
    clipped_samples: u64,
    discontinuities: DiscontinuityMeasurement,
) -> Measurements {
    let (integrated, true_peak, loudness_range) = loudness(samples, sample_rate_hz, channels);
    let loudness_approx = short_window_loudness_range(samples, sample_rate_hz, channels);
    let spectrum = spectral_measurements(samples, sample_rate_hz, channels);
    Measurements {
        integrated_lufs: integrated,
        true_peak_dbtp: true_peak,
        loudness_range_lu: loudness_range,
        short_window_loudness_range_approx_lu: loudness_approx,
        spectral_centroid_hz: spectrum.centroid,
        high_frequency_energy_ratio: spectrum.high_frequency_ratio,
        onset_density_per_second: spectrum.onset_density,
        silence,
        clipped_samples,
        non_finite_samples: 0,
        discontinuity_candidates: discontinuities,
    }
}

fn unavailable_metrics(
    message: &str,
    silence: SilenceMeasurement,
    clipped_samples: u64,
    non_finite_samples: u64,
    discontinuities: DiscontinuityMeasurement,
) -> Measurements {
    Measurements {
        integrated_lufs: NumericMetric::unavailable("LUFS", "EBU R128 integrated", message),
        true_peak_dbtp: NumericMetric::unavailable("dBTP", "EBU R128 true peak", message),
        loudness_range_lu: NumericMetric::unavailable("LU", "EBU Tech 3342 LRA", message),
        short_window_loudness_range_approx_lu: NumericMetric::unavailable(
            "LU",
            "ungated 400 ms channel-aggregated power P95-P10 approximation; not EBU LRA",
            message,
        ),
        spectral_centroid_hz: NumericMetric::unavailable(
            "Hz",
            "channel-summed power centroid from independent 2048-point Hann STFTs",
            message,
        ),
        high_frequency_energy_ratio: NumericMetric::unavailable(
            "ratio",
            "channel-summed STFT power at or above 8 kHz divided by total power",
            message,
        ),
        onset_density_per_second: NumericMetric::unavailable(
            "events/second",
            "positive flux of L1-normalized channel-summed power spectra with max(median + 3 MAD, 0.02) threshold",
            message,
        ),
        silence,
        clipped_samples,
        non_finite_samples,
        discontinuity_candidates: discontinuities,
    }
}

fn loudness(
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
) -> (NumericMetric, NumericMetric, NumericMetric) {
    let method_i = "EBU R128 integrated (ebur128 0.1.10)";
    let method_tp = "EBU R128 true peak, implementation-defined oversampling (ebur128 0.1.10)";
    let method_lra = "EBU Tech 3342 LRA (ebur128 0.1.10)";
    let mode = Mode::I | Mode::LRA | Mode::TRUE_PEAK;
    let mut meter = match EbuR128::new(u32::from(channels), sample_rate_hz, mode) {
        Ok(meter) => meter,
        Err(error) => {
            let message = error.to_string();
            return (
                NumericMetric::unavailable("LUFS", method_i, &message),
                NumericMetric::unavailable("dBTP", method_tp, &message),
                NumericMetric::unavailable("LU", method_lra, message),
            );
        }
    };
    if let Err(error) = meter.add_frames_f32(samples) {
        let message = error.to_string();
        return (
            NumericMetric::unavailable("LUFS", method_i, &message),
            NumericMetric::unavailable("dBTP", method_tp, &message),
            NumericMetric::unavailable("LU", method_lra, message),
        );
    }
    let integrated = match meter.loudness_global() {
        Ok(value) if value.is_finite() => NumericMetric::measured(value, "LUFS", method_i),
        Ok(_) => NumericMetric::unavailable("LUFS", method_i, "meter returned a non-finite value"),
        Err(error) => NumericMetric::unavailable("LUFS", method_i, error.to_string()),
    };
    let peak = (0..u32::from(channels))
        .filter_map(|channel| meter.true_peak(channel).ok())
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max);
    let true_peak = if peak > 0.0 {
        NumericMetric::measured(20.0 * peak.log10(), "dBTP", method_tp)
    } else {
        NumericMetric::unavailable("dBTP", method_tp, "digital silence has no finite dB peak")
    };
    let duration_seconds = samples.len() as f64 / f64::from(channels) / f64::from(sample_rate_hz);
    let loudness_range = if duration_seconds < 3.0 {
        NumericMetric::unavailable(
            "LU",
            method_lra,
            "EBU loudness range requires at least 3 seconds of programme history",
        )
    } else {
        match meter.loudness_range() {
            Ok(value) if value.is_finite() => NumericMetric::measured(value, "LU", method_lra),
            Ok(_) => {
                NumericMetric::unavailable("LU", method_lra, "meter returned a non-finite value")
            }
            Err(error) => NumericMetric::unavailable("LU", method_lra, error.to_string()),
        }
    };
    (integrated, true_peak, loudness_range)
}

fn short_window_loudness_range(
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
) -> NumericMetric {
    let method = "ungated 400 ms channel-aggregated power P95-P10 approximation; not EBU LRA";
    let window = ((u64::from(sample_rate_hz) * 400) / 1_000) as usize;
    let hop = ((u64::from(sample_rate_hz) * 100) / 1_000) as usize;
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    if window == 0 || hop == 0 || frames < window {
        return NumericMetric::unavailable(
            "LU",
            method,
            "source is shorter than the 400 ms analysis window",
        );
    }
    let mut levels = (0..=frames - window)
        .step_by(hop)
        .filter_map(|start| {
            let sample_start = start * channels;
            let sample_end = (start + window) * channels;
            let window_samples = &samples[sample_start..sample_end];
            let mean_square = window_samples
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / window_samples.len() as f64;
            (mean_square > 0.0).then(|| 10.0 * mean_square.log10())
        })
        .collect::<Vec<_>>();
    if levels.is_empty() {
        return NumericMetric::unavailable("LU", method, "all windows are digital silence");
    }
    levels.sort_by(f64::total_cmp);
    let low = percentile(&levels, 0.10);
    let high = percentile(&levels, 0.95);
    NumericMetric::measured(high - low, "LU", method)
}

struct SpectrumMeasurements {
    centroid: NumericMetric,
    high_frequency_ratio: NumericMetric,
    onset_density: NumericMetric,
}

fn spectral_measurements(
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
) -> SpectrumMeasurements {
    let centroid_method = "channel-summed power centroid from independent 2048-point Hann STFTs";
    let high_method = "channel-summed STFT power at or above 8 kHz divided by total power";
    let onset_method = "positive flux of L1-normalized channel-summed power spectra with max(median + 3 MAD, 0.02) threshold";
    if samples.is_empty() || sample_rate_hz == 0 || channels == 0 {
        return SpectrumMeasurements {
            centroid: NumericMetric::unavailable("Hz", centroid_method, "no PCM frames"),
            high_frequency_ratio: NumericMetric::unavailable("ratio", high_method, "no PCM frames"),
            onset_density: NumericMetric::unavailable(
                "events/second",
                onset_method,
                "no PCM frames",
            ),
        };
    }
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(SPECTRUM_WINDOW);
    let hann = (0..SPECTRUM_WINDOW)
        .map(|index| {
            0.5 - 0.5
                * (2.0 * std::f64::consts::PI * index as f64 / (SPECTRUM_WINDOW - 1) as f64).cos()
        })
        .collect::<Vec<_>>();
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    let starts = if frames < SPECTRUM_WINDOW {
        vec![0]
    } else {
        (0..=frames - SPECTRUM_WINDOW)
            .step_by(SPECTRUM_HOP)
            .collect()
    };
    let bins = SPECTRUM_WINDOW / 2 + 1;
    let mut total_power = 0.0;
    let mut weighted_frequency = 0.0;
    let mut high_power = 0.0;
    let mut previous_magnitudes: Option<Vec<f64>> = None;
    let mut fluxes = Vec::new();
    for start in starts {
        let mut channel_power = vec![0.0; bins];
        for channel in 0..channels {
            let mut buffer = (0..SPECTRUM_WINDOW)
                .map(|offset| Complex {
                    re: samples
                        .get((start + offset) * channels + channel)
                        .map_or(0.0, |sample| f64::from(*sample))
                        * hann[offset],
                    im: 0.0,
                })
                .collect::<Vec<_>>();
            fft.process(&mut buffer);
            for (power, value) in channel_power.iter_mut().zip(&buffer[..bins]) {
                *power += value.norm_sqr();
            }
        }
        let power_sum = channel_power.iter().sum::<f64>();
        let normalized_power = if power_sum > 0.0 {
            channel_power
                .iter()
                .map(|power| power / power_sum)
                .collect::<Vec<_>>()
        } else {
            vec![0.0; channel_power.len()]
        };
        if let Some(previous) = &previous_magnitudes {
            fluxes.push(
                normalized_power
                    .iter()
                    .zip(previous)
                    .map(|(current, previous)| (current - previous).max(0.0))
                    .sum::<f64>(),
            );
        }
        for (bin, power) in channel_power.iter().enumerate().skip(1) {
            let frequency = bin as f64 * f64::from(sample_rate_hz) / SPECTRUM_WINDOW as f64;
            total_power += *power;
            weighted_frequency += frequency * *power;
            if frequency >= HIGH_FREQUENCY_HZ {
                high_power += *power;
            }
        }
        previous_magnitudes = Some(normalized_power);
    }
    let (centroid, high_frequency_ratio) = if total_power > 0.0 {
        (
            NumericMetric::measured(weighted_frequency / total_power, "Hz", centroid_method),
            NumericMetric::measured(high_power / total_power, "ratio", high_method),
        )
    } else {
        (
            NumericMetric::unavailable("Hz", centroid_method, "digital silence has no spectrum"),
            NumericMetric::unavailable("ratio", high_method, "digital silence has no spectrum"),
        )
    };
    let duration = frames as f64 / f64::from(sample_rate_hz);
    let onset_count = if fluxes.is_empty() {
        0
    } else {
        let median_flux = median(&fluxes);
        let deviations = fluxes
            .iter()
            .map(|value| (value - median_flux).abs())
            .collect::<Vec<_>>();
        let threshold = (median_flux + 3.0 * median(&deviations)).max(0.02);
        fluxes
            .iter()
            .filter(|flux| **flux > threshold && **flux > 1e-9)
            .count()
    };
    SpectrumMeasurements {
        centroid,
        high_frequency_ratio,
        onset_density: NumericMetric::measured(
            onset_count as f64 / duration,
            "events/second",
            onset_method,
        ),
    }
}

fn time_domain_measurements(
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
) -> (SilenceMeasurement, DiscontinuityMeasurement) {
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    let mut silent_frames = 0usize;
    let mut longest_silent_frames = 0usize;
    let mut run_start = None;
    let mut regions = Vec::new();
    for (frame_index, frame) in samples.chunks_exact(channels).enumerate() {
        let silent = frame
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= SILENCE_AMPLITUDE);
        if silent {
            silent_frames += 1;
            run_start.get_or_insert(frame_index);
        } else if let Some(start) = run_start.take() {
            record_silence_region(start, frame_index, sample_rate_hz, &mut regions);
            longest_silent_frames = longest_silent_frames.max(frame_index - start);
        }
    }
    if let Some(start) = run_start {
        record_silence_region(start, frames, sample_rate_hz, &mut regions);
        longest_silent_frames = longest_silent_frames.max(frames - start);
    }
    let silence = SilenceMeasurement {
        threshold_amplitude: round_six(f64::from(SILENCE_AMPLITUDE)),
        total_near_silence_seconds: round_six(silent_frames as f64 / f64::from(sample_rate_hz)),
        longest_near_silence_seconds: round_six(
            longest_silent_frames as f64 / f64::from(sample_rate_hz),
        ),
        regions,
        method: "all channels at or below absolute amplitude 0.0001; regions report >=100 ms",
    };

    let mut candidate_count = 0u64;
    let mut candidate_times = Vec::new();
    let mut max_delta = 0.0f32;
    for frame_index in 1..frames {
        let current = &samples[frame_index * channels..(frame_index + 1) * channels];
        let previous = &samples[(frame_index - 1) * channels..frame_index * channels];
        let frame_delta = current
            .iter()
            .zip(previous)
            .filter(|(current, previous)| current.is_finite() && previous.is_finite())
            .map(|(current, previous)| (current - previous).abs())
            .fold(0.0f32, f32::max);
        max_delta = max_delta.max(frame_delta);
        if frame_delta >= CLICK_DELTA {
            candidate_count += 1;
            if candidate_times.len() < 32 {
                candidate_times.push(round_six(frame_index as f64 / f64::from(sample_rate_hz)));
            }
        }
    }
    let discontinuities = DiscontinuityMeasurement {
        threshold_inter_sample_delta: f64::from(CLICK_DELTA),
        candidate_count,
        maximum_inter_sample_delta: round_six(f64::from(max_delta)),
        candidate_times_seconds: candidate_times,
        candidate_times_truncated: candidate_count > 32,
        method: "maximum same-channel absolute delta between adjacent frames; candidates are not proof of audible clicks",
    };
    (silence, discontinuities)
}

fn record_silence_region(
    start: usize,
    end: usize,
    sample_rate_hz: u32,
    regions: &mut Vec<TimeRegion>,
) {
    let duration = (end - start) as f64 / f64::from(sample_rate_hz);
    if duration >= REPORTABLE_SILENCE_SECONDS {
        regions.push(TimeRegion {
            start_seconds: round_six(start as f64 / f64::from(sample_rate_hz)),
            end_seconds: round_six(end as f64 / f64::from(sample_rate_hz)),
        });
    }
}

fn calibration_flags(measurements: &Measurements) -> Vec<Reason> {
    let mut flags = Vec::new();
    if measurements.discontinuity_candidates.candidate_count > 0 {
        flags.push(reason(
            "discontinuity_candidates_require_review",
            "one or more raw adjacent-sample jumps meet the provisional candidate threshold; this is not proof of an audible click",
        ));
    }
    if measurements
        .integrated_lufs
        .value
        .is_some_and(|value| !(-28.0..=-14.0).contains(&value))
    {
        flags.push(reason(
            "integrated_loudness_outside_calibration_range",
            "integrated loudness is outside the provisional -28 to -14 LUFS range",
        ));
    }
    if measurements
        .true_peak_dbtp
        .value
        .is_some_and(|value| value > -1.0)
    {
        flags.push(reason(
            "true_peak_above_calibration_ceiling",
            "true peak exceeds the provisional -1 dBTP ceiling",
        ));
    }
    let range = measurements
        .loudness_range_lu
        .value
        .or(measurements.short_window_loudness_range_approx_lu.value);
    if range.is_some_and(|value| value > 8.0) {
        flags.push(reason(
            "loudness_range_above_calibration_range",
            "measured loudness range exceeds the provisional 8 LU flag threshold",
        ));
    }
    if measurements
        .high_frequency_energy_ratio
        .value
        .is_some_and(|value| value > 0.25)
    {
        flags.push(reason(
            "high_frequency_energy_above_calibration_range",
            "energy at or above 8 kHz exceeds the provisional 0.25 ratio threshold",
        ));
    }
    if measurements
        .onset_density_per_second
        .value
        .is_some_and(|value| value > 4.0)
    {
        flags.push(reason(
            "onset_density_above_calibration_range",
            "onset density exceeds the provisional 4 events/second threshold",
        ));
    }
    flags
}

fn decode_file(path: &Path) -> Result<DecodedAsset, DecodeFailure> {
    decode_snapshot(snapshot_source(path)?)
}

fn snapshot_source(path: &Path) -> Result<SourceSnapshot, DecodeFailure> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<non-utf8>")
        .to_owned();
    let codec = Codec::from_path(path);
    let mut source = SourceIdentity {
        file_name,
        bytes: None,
        sha256: None,
    };
    let codec = codec.ok_or_else(|| DecodeFailure {
        source: source.clone(),
        codec: None,
        code: "unsupported_extension",
        message: "only .wav, .flac, and .mp3 candidates are supported".to_owned(),
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "unsafe_or_unreadable_source",
        message: error.to_string(),
    })?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(DecodeFailure {
            source,
            codec: Some(codec),
            code: "unsafe_or_unreadable_source",
            message: "source is linked/reparse-backed or is not a regular file".to_owned(),
        });
    }
    source.bytes = Some(metadata.len());
    if metadata.len() == 0 || metadata.len() > MAX_ENCODED_BYTES {
        return Err(DecodeFailure {
            source,
            codec: Some(codec),
            code: "encoded_size_limit",
            message: format!("source must contain 1..={MAX_ENCODED_BYTES} bytes"),
        });
    }
    let mut file = File::open(path).map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "unsafe_or_unreadable_source",
        message: error.to_string(),
    })?;
    let handle_metadata = file.metadata().map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "unsafe_or_unreadable_source",
        message: error.to_string(),
    })?;
    if !handle_metadata.is_file() || handle_metadata.len() != metadata.len() {
        return Err(DecodeFailure {
            source,
            codec: Some(codec),
            code: "source_changed_during_open",
            message: "opened handle metadata differs from the inspected source".to_owned(),
        });
    }
    let mut snapshot = NamedTempFile::new().map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "snapshot_creation_failure",
        message: error.to_string(),
    })?;
    let mut hasher = Sha256::new();
    let mut remaining = metadata.len();
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let request = remaining.min(buffer.len() as u64) as usize;
        let count = file
            .read(&mut buffer[..request])
            .map_err(|error| DecodeFailure {
                source: source.clone(),
                codec: Some(codec),
                code: "read_failure",
                message: error.to_string(),
            })?;
        if count == 0 {
            return Err(DecodeFailure {
                source,
                codec: Some(codec),
                code: "source_changed_during_read",
                message: "source ended before its inspected byte length".to_owned(),
            });
        }
        hasher.update(&buffer[..count]);
        snapshot
            .as_file_mut()
            .write_all(&buffer[..count])
            .map_err(|error| DecodeFailure {
                source: source.clone(),
                codec: Some(codec),
                code: "snapshot_write_failure",
                message: error.to_string(),
            })?;
        remaining -= count as u64;
    }
    let mut extra = [0u8; 1];
    if file.read(&mut extra).map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "read_failure",
        message: error.to_string(),
    })? != 0
    {
        return Err(DecodeFailure {
            source,
            codec: Some(codec),
            code: "source_changed_during_snapshot",
            message: "source grew while its immutable analysis snapshot was being created"
                .to_owned(),
        });
    }
    snapshot
        .as_file_mut()
        .sync_all()
        .map_err(|error| DecodeFailure {
            source: source.clone(),
            codec: Some(codec),
            code: "snapshot_write_failure",
            message: error.to_string(),
        })?;
    source.sha256 = Some(format!("{:x}", hasher.finalize()));
    Ok(SourceSnapshot {
        source,
        codec,
        file: snapshot,
    })
}

fn decode_snapshot(snapshot: SourceSnapshot) -> Result<DecodedAsset, DecodeFailure> {
    let source = snapshot.source;
    let codec = snapshot.codec;
    let mut file = snapshot.file.reopen().map_err(|error| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code: "snapshot_read_failure",
        message: error.to_string(),
    })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| DecodeFailure {
            source: source.clone(),
            codec: Some(codec),
            code: "snapshot_read_failure",
            message: error.to_string(),
        })?;
    decode_open_file(file, source, codec)
}

fn decode_open_file(
    file: File,
    source: SourceIdentity,
    codec: Codec,
) -> Result<DecodedAsset, DecodeFailure> {
    let fail = |code: &'static str, message: String| DecodeFailure {
        source: source.clone(),
        codec: Some(codec),
        code,
        message,
    };
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(codec.extension());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| fail("probe_or_corruption_failure", error.to_string()))?;
    if format.format_info().short_name != codec.format_name() {
        return Err(fail(
            "codec_mismatch",
            format!(
                "extension declares {} but probe detected {}",
                codec.extension(),
                format.format_info().short_name
            ),
        ));
    }
    let (track_id, params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| fail("missing_audio_track", "no default audio track".to_owned()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| {
                fail(
                    "missing_codec_parameters",
                    "audio track has no codec parameters".to_owned(),
                )
            })?
            .clone();
        (track.id, params)
    };
    if !codec.accepts(params.codec) {
        return Err(fail(
            "unsupported_or_mismatched_codec",
            "detected codec is not an accepted WAV PCM, FLAC, or MP3 stream".to_owned(),
        ));
    }
    let mut sample_rate_hz = params.sample_rate;
    let mut channels = params
        .channels
        .as_ref()
        .map(|channels| channels.count() as u16);
    let bit_depth = params.bits_per_sample.map(|bits| bits as u16);
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|error| fail("decoder_initialization_failure", error.to_string()))?;
    let mut samples = Vec::new();
    while let Some(packet) = format
        .next_packet()
        .map_err(|error| fail("decode_or_corruption_failure", error.to_string()))?
    {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|error| fail("decode_or_corruption_failure", error.to_string()))?;
        let spec = decoded.spec();
        let packet_rate = spec.rate();
        let packet_channels = spec.channels().count() as u16;
        if sample_rate_hz.is_some_and(|rate| rate != packet_rate)
            || channels.is_some_and(|count| count != packet_channels)
        {
            return Err(fail(
                "inconsistent_stream_metadata",
                "sample rate or channel count changes within the stream".to_owned(),
            ));
        }
        sample_rate_hz = Some(packet_rate);
        channels = Some(packet_channels);
        if !matches!(packet_channels, 1 | 2) {
            return Err(fail(
                "unsupported_channel_layout",
                format!("only mono/stereo are supported; detected {packet_channels} channels"),
            ));
        }
        let packet_samples = decoded.samples_interleaved();
        let next_len = samples
            .len()
            .checked_add(packet_samples)
            .filter(|length| *length <= MAX_DECODED_SAMPLES)
            .ok_or_else(|| {
                fail(
                    "decoded_sample_limit",
                    format!("decoded PCM exceeds {MAX_DECODED_SAMPLES} samples"),
                )
            })?;
        let start = samples.len();
        samples.resize(next_len, 0.0f32);
        decoded.copy_to_slice_interleaved(&mut samples[start..]);
    }
    let sample_rate_hz = sample_rate_hz.filter(|rate| *rate > 0).ok_or_else(|| {
        fail(
            "missing_sample_rate",
            "decoder reported no sample rate".to_owned(),
        )
    })?;
    let channels = channels
        .filter(|channels| matches!(channels, 1 | 2))
        .ok_or_else(|| {
            fail(
                "missing_channel_count",
                "decoder reported no mono/stereo layout".to_owned(),
            )
        })?;
    if samples.is_empty() {
        return Err(fail(
            "empty_decode",
            "decoder produced no PCM samples".to_owned(),
        ));
    }
    if !samples.len().is_multiple_of(usize::from(channels)) {
        return Err(fail(
            "unaligned_decode",
            "decoded samples are not channel-aligned".to_owned(),
        ));
    }
    Ok(DecodedAsset {
        source,
        codec,
        sample_rate_hz,
        channels,
        bit_depth,
        samples,
    })
}

fn failure_report(failure: DecodeFailure) -> AnalysisReport {
    let message = failure.message;
    AnalysisReport {
        schema_version: 1,
        analyzer: analyzer_identity(),
        source: failure.source,
        decode: DecodeReport {
            status: DecodeStatus::Failed,
            codec: failure.codec,
            sample_rate_hz: None,
            channels: None,
            bit_depth: None,
            frames: None,
            duration_seconds: None,
            error_code: Some(failure.code.to_owned()),
            error_message: Some(message.clone()),
        },
        measurements: empty_measurements("decode did not produce analyzable PCM"),
        unassessed: unassessed(),
        hard_rejections: vec![reason(failure.code, message)],
        flags: Vec::new(),
    }
}

fn empty_measurements(reason: &str) -> Measurements {
    let silence = SilenceMeasurement {
        threshold_amplitude: round_six(f64::from(SILENCE_AMPLITUDE)),
        total_near_silence_seconds: 0.0,
        longest_near_silence_seconds: 0.0,
        regions: Vec::new(),
        method: "not run because decoding failed",
    };
    let discontinuities = DiscontinuityMeasurement {
        threshold_inter_sample_delta: f64::from(CLICK_DELTA),
        candidate_count: 0,
        maximum_inter_sample_delta: 0.0,
        candidate_times_seconds: Vec::new(),
        candidate_times_truncated: false,
        method: "not run because decoding failed",
    };
    unavailable_metrics(reason, silence, 0, 0, discontinuities)
}

fn analyzer_identity() -> AnalyzerIdentity {
    AnalyzerIdentity {
        name: "adhd-music-audio-analyzer",
        version: env!("CARGO_PKG_VERSION"),
        deterministic_contract:
            "same analyzer version, bytes, and platform produce field-stable rounded JSON values",
    }
}

fn unassessed() -> UnassessedMeasurements {
    let tempo = NotAssessed {
        status: "not_assessed",
        reason: "no validated tempo estimator is implemented in schema version 1",
    };
    UnassessedMeasurements {
        tempo_bpm: tempo.clone(),
        tempo_confidence: tempo.clone(),
        tempo_drift_percent: tempo,
        section_change_novelty: NotAssessed {
            status: "not_assessed",
            reason: "no calibrated structural-segmentation model is implemented in schema version 1",
        },
        vocal_speech_likelihood: NotAssessed {
            status: "not_assessed",
            reason: "spectral heuristics are not a validated speech or singing detector; human review remains required",
        },
    }
}

fn reason(code: &'static str, message: impl Into<String>) -> Reason {
    Reason {
        code,
        message: message.into(),
    }
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    let index = ((values.len() - 1) as f64 * quantile).round() as usize;
    values[index]
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    if values.len().is_multiple_of(2) {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) * 0.5
    } else {
        values[values.len() / 2]
    }
}

fn round_six(value: f64) -> f64 {
    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    if rounded == -0.0 {
        0.0
    } else {
        rounded
    }
}

fn pcm_bit_depth(codec: AudioCodecId) -> Option<u16> {
    match codec {
        CODEC_ID_PCM_S16LE | CODEC_ID_PCM_S16BE | CODEC_ID_PCM_U16LE | CODEC_ID_PCM_U16BE => {
            Some(16)
        }
        CODEC_ID_PCM_S24LE | CODEC_ID_PCM_S24BE | CODEC_ID_PCM_U24LE | CODEC_ID_PCM_U24BE => {
            Some(24)
        }
        CODEC_ID_PCM_S32LE | CODEC_ID_PCM_S32BE | CODEC_ID_PCM_U32LE | CODEC_ID_PCM_U32BE
        | CODEC_ID_PCM_F32LE | CODEC_ID_PCM_F32BE => Some(32),
        _ => None,
    }
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn usage(program: &str) -> String {
    format!("Usage: {program} --input <candidate.wav|flac|mp3> [--output <report.json>]")
}

pub fn parse_args(
    args: impl IntoIterator<Item = String>,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut arguments = args.into_iter();
    let program = arguments
        .next()
        .unwrap_or_else(|| "audio-analyzer".to_owned());
    let mut input = None;
    let mut output = None;
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(|| usage(&program))?;
        match flag.as_str() {
            "--input" if input.is_none() => input = Some(PathBuf::from(value)),
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(usage(&program)),
        }
    }
    input
        .map(|input| (input, output))
        .ok_or_else(|| usage(&program))
}

/// Atomically publish a complete report without replacing an existing path.
pub fn write_report_noclobber(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    sync_parent_directory(path)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    // The report file itself is synced. Opening a Windows directory for sync
    // requires FILE_FLAG_BACKUP_SEMANTICS, so the atomic rename is the fallback.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/audio-engine/tests/fixtures")
            .join(name)
    }

    fn wav_bytes(rate: u32, samples: &[f32]) -> Vec<u8> {
        wav_bytes_with_channels(rate, 1, samples)
    }

    fn wav_bytes_with_channels(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
        let data_bytes = (samples.len() * 4) as u32;
        let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
        bytes.extend_from_slice(&(channels * 4).to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn committed_codec_matrix_decodes_with_truthful_metadata() {
        let cases = [
            ("wav_pcm16_mono_44100.wav", Codec::Wav, 44_100, 1, Some(16)),
            (
                "wav_pcm24_stereo_48000.wav",
                Codec::Wav,
                48_000,
                2,
                Some(24),
            ),
            ("wav_f32_stereo_96000.wav", Codec::Wav, 96_000, 2, Some(32)),
            ("flac_mono_44100.flac", Codec::Flac, 44_100, 1, Some(16)),
            ("flac_stereo_48000.flac", Codec::Flac, 48_000, 2, Some(16)),
            ("mp3_mono_44100.mp3", Codec::Mp3, 44_100, 1, None),
            ("mp3_stereo_48000.mp3", Codec::Mp3, 48_000, 2, None),
        ];
        for (name, codec, rate, channels, bit_depth) in cases {
            let report = analyze_file(&fixture(name));
            assert_eq!(report.decode.status, DecodeStatus::Decoded, "{name}");
            assert_eq!(report.decode.codec, Some(codec), "{name}");
            assert_eq!(report.decode.sample_rate_hz, Some(rate), "{name}");
            assert_eq!(report.decode.channels, Some(channels), "{name}");
            assert_eq!(report.decode.bit_depth, bit_depth, "{name}");
            assert_eq!(report.decode.duration_seconds, Some(1.0), "{name}");
            assert_eq!(report.measurements.non_finite_samples, 0, "{name}");
            assert_eq!(
                report.measurements.integrated_lufs.status,
                MetricStatus::Measured
            );
            assert_eq!(
                report.measurements.true_peak_dbtp.status,
                MetricStatus::Measured
            );
            assert_eq!(
                report.measurements.loudness_range_lu.status,
                MetricStatus::Unavailable,
                "short fixture LRA must not be fabricated for {name}"
            );
            assert_eq!(
                report
                    .measurements
                    .short_window_loudness_range_approx_lu
                    .status,
                MetricStatus::Measured,
                "{name}"
            );
        }
    }

    #[test]
    fn output_is_deterministic_and_unimplemented_inference_is_explicit() {
        let path = fixture("wav_pcm16_mono_44100.wav");
        let first = serde_json::to_string_pretty(&analyze_file(&path)).unwrap();
        let second = serde_json::to_string_pretty(&analyze_file(&path)).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"status\": \"not_assessed\""));
        assert!(first.contains("spectral heuristics are not a validated speech"));
    }

    #[test]
    fn silence_and_clipping_are_hard_but_discontinuities_are_provisional_flags() {
        let temp = tempfile::tempdir().unwrap();
        let silence = analyze_file(&fixture("wav_zero_mono_44100.wav"));
        assert!(silence
            .hard_rejections
            .iter()
            .any(|reason| reason.code == "unexplained_near_silence"));
        assert_eq!(
            silence.measurements.silence.longest_near_silence_seconds,
            1.0
        );

        let samples = [0.0, 1.0, -1.0, 0.0];
        let path = temp.path().join("defects.wav");
        fs::write(&path, wav_bytes(8_000, &samples)).unwrap();
        let defects = analyze_file(&path);
        assert_eq!(defects.measurements.clipped_samples, 2);
        assert!(
            defects
                .measurements
                .discontinuity_candidates
                .candidate_count
                >= 2
        );
        assert!(defects
            .hard_rejections
            .iter()
            .any(|reason| reason.code == "clipped_samples"));
        assert!(!defects
            .hard_rejections
            .iter()
            .any(|reason| reason.code.contains("discontinuity")));
        assert_eq!(
            defects
                .flags
                .iter()
                .filter(|reason| reason.code == "discontinuity_candidates_require_review")
                .count(),
            1
        );
    }

    #[test]
    fn non_finite_and_corrupt_inputs_emit_structured_failure_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let nan_path = temp.path().join("nan.wav");
        fs::write(&nan_path, wav_bytes(8_000, &[0.0, f32::NAN, 0.0])).unwrap();
        let nan = analyze_file(&nan_path);
        assert_eq!(nan.decode.status, DecodeStatus::Decoded);
        assert_eq!(nan.measurements.non_finite_samples, 1);
        assert!(nan
            .hard_rejections
            .iter()
            .any(|reason| reason.code == "non_finite_samples"));

        let corrupt_path = temp.path().join("corrupt.wav");
        fs::write(&corrupt_path, [0u8; 64]).unwrap();
        let corrupt = analyze_file(&corrupt_path);
        assert_eq!(corrupt.decode.status, DecodeStatus::Failed);
        assert!(corrupt.decode.error_code.is_some());
        assert_eq!(corrupt.hard_rejections.len(), 1);
        serde_json::to_string_pretty(&corrupt).unwrap();
    }

    #[test]
    fn stable_sine_spectrum_is_near_fixture_frequency() {
        let report = analyze_file(&fixture("wav_pcm16_mono_44100.wav"));
        let centroid = report.measurements.spectral_centroid_hz.value.unwrap();
        assert!((centroid - 220.0).abs() < 10.0, "centroid was {centroid}");
        assert_eq!(
            report.measurements.onset_density_per_second.value,
            Some(0.0)
        );
    }

    #[test]
    fn anti_phase_stereo_cannot_cancel_channel_energy_analysis() {
        let temp = tempfile::tempdir().unwrap();
        let rate = 44_100;
        let frequency = 10_000.0_f32;
        let samples = (0..rate)
            .flat_map(|frame| {
                let phase = 2.0 * std::f32::consts::PI * frequency * frame as f32 / rate as f32;
                let value = 0.2 * phase.sin();
                [value, -value]
            })
            .collect::<Vec<_>>();
        let path = temp.path().join("anti-phase.wav");
        fs::write(&path, wav_bytes_with_channels(rate, 2, &samples)).unwrap();

        let report = analyze_file(&path);
        assert_eq!(report.decode.status, DecodeStatus::Decoded);
        assert_eq!(report.decode.channels, Some(2));
        let centroid = report.measurements.spectral_centroid_hz.value.unwrap();
        assert!(
            (centroid - 10_000.0).abs() < 20.0,
            "centroid was {centroid}"
        );
        assert!(report
            .measurements
            .high_frequency_energy_ratio
            .value
            .is_some_and(|ratio| ratio > 0.99));
        assert_eq!(
            report
                .measurements
                .short_window_loudness_range_approx_lu
                .status,
            MetricStatus::Measured
        );
        assert_eq!(
            report.measurements.integrated_lufs.status,
            MetricStatus::Measured
        );
    }

    #[test]
    fn immutable_snapshot_excludes_bytes_appended_to_live_source() {
        let temp = tempfile::tempdir().unwrap();
        let samples = (0..8_000)
            .map(|frame| (frame as f32 * 0.031).sin() * 0.2)
            .collect::<Vec<_>>();
        let bytes = wav_bytes(8_000, &samples);
        let path = temp.path().join("source.wav");
        fs::write(&path, &bytes).unwrap();
        let snapshot = snapshot_source(&path).unwrap();

        let mut live_source = fs::OpenOptions::new().append(true).open(&path).unwrap();
        live_source
            .write_all(b"bytes appended after snapshot")
            .unwrap();
        live_source.sync_all().unwrap();

        let decoded = decode_snapshot(snapshot).unwrap();
        assert_eq!(decoded.source.bytes, Some(bytes.len() as u64));
        assert_eq!(
            decoded.source.sha256,
            Some(format!("{:x}", Sha256::digest(&bytes)))
        );
        assert_eq!(decoded.samples.len(), samples.len());
        assert!(fs::metadata(path).unwrap().len() > bytes.len() as u64);
    }

    #[test]
    fn report_publication_is_atomic_and_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("report.json");
        write_report_noclobber(&path, b"complete report\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"complete report\n");

        let error = write_report_noclobber(&path, b"replacement").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(path).unwrap(), b"complete report\n");
    }

    #[test]
    fn argument_parser_is_strict() {
        assert_eq!(
            parse_args([
                "audio-analyzer".to_owned(),
                "--input".to_owned(),
                "candidate.flac".to_owned(),
                "--output".to_owned(),
                "report.json".to_owned(),
            ])
            .unwrap(),
            (
                PathBuf::from("candidate.flac"),
                Some(PathBuf::from("report.json"))
            )
        );
        assert!(parse_args(["audio-analyzer".to_owned(), "--input".to_owned()]).is_err());
    }
}
