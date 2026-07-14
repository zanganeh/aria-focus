//! Bounded, whole-file media preparation. Every operation in this module runs
//! before a CPAL callback is built; the callback only receives `DeviceProgram`.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;

use opus2::{Channels as OpusChannels, Decoder as OpusDecoder};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use sha2::{Digest, Sha256};
use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS, CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE,
    CODEC_ID_PCM_S16BE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32BE, CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U16BE, CODEC_ID_PCM_U16LE,
    CODEC_ID_PCM_U24BE, CODEC_ID_PCM_U24LE, CODEC_ID_PCM_U32BE, CODEC_ID_PCM_U32LE,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const MAX_ENCODED_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PROGRAM_SAMPLES: usize = 64 * 1024 * 1024;
pub const MAX_DEVICE_PROGRAM_SAMPLES: usize = 64 * 1024 * 1024;
pub const MAX_PROGRAM_TRACKS: usize = 8;
pub const MAX_DURATION_MISMATCH_SECONDS: f64 = 0.150;
const RESAMPLER_CHUNK_FRAMES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCodec {
    Wav,
    Flac,
    Mp3,
    OggOpus,
}

impl MediaCodec {
    fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::OggOpus => "opus",
        }
    }

    fn accepts(self, codec: AudioCodecId) -> bool {
        match self {
            Self::Wav => pcm_bit_depth(codec).is_some_and(|actual| matches!(actual, 16 | 24 | 32)),
            Self::Flac => codec == CODEC_ID_FLAC,
            Self::Mp3 => codec == CODEC_ID_MP3,
            Self::OggOpus => codec == CODEC_ID_OPUS,
        }
    }

    fn format_name(self) -> &'static str {
        match self {
            Self::Wav => "wave",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::OggOpus => "ogg",
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoredRegionKind {
    Loop,
    Crossfade,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredRegion {
    pub kind: AuthoredRegionKind,
    pub start_seconds: f32,
    pub end_seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLabel {
    pub pack_id: String,
    pub pack_title: String,
    pub item_id: String,
    pub item_title: String,
    pub variant_id: String,
}

impl SourceLabel {
    pub fn test_fallback() -> Self {
        Self {
            pack_id: "bundled-test-source".to_owned(),
            pack_title: "Bundled fallback".to_owned(),
            item_id: "procedural-focus-tone".to_owned(),
            item_title: "Procedural test tone".to_owned(),
            variant_id: "deterministic-v1".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodeExpectation {
    pub path: PathBuf,
    pub codec: MediaCodec,
    pub bytes: u64,
    pub sha256: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bit_depth: Option<u16>,
    pub duration_seconds: f32,
    pub regions: Vec<AuthoredRegion>,
    pub label: SourceLabel,
}

#[derive(Debug, Clone)]
pub struct DecodedTrack {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
    pub regions: Vec<AuthoredRegion>,
    pub label: SourceLabel,
}

#[derive(Debug, Clone)]
pub struct DecodedProgram {
    pub tracks: Vec<DecodedTrack>,
}

impl DecodedProgram {
    pub fn new(tracks: Vec<DecodedTrack>) -> Result<Self, MediaError> {
        Self::with_sample_limit(tracks, MAX_PROGRAM_SAMPLES)
    }

    fn with_sample_limit(
        tracks: Vec<DecodedTrack>,
        sample_limit: usize,
    ) -> Result<Self, MediaError> {
        if tracks.is_empty() || tracks.len() > MAX_PROGRAM_TRACKS {
            return Err(MediaError::InvalidProgramSize(tracks.len()));
        }
        let mut total = 0usize;
        for track in &tracks {
            let channels = usize::from(track.channels);
            if track.sample_rate_hz == 0
                || !matches!(track.channels, 1 | 2)
                || track.samples.is_empty()
                || !track.samples.len().is_multiple_of(channels)
                || track.samples.iter().any(|sample| !sample.is_finite())
            {
                return Err(MediaError::InvalidDecodedLayout);
            }
            total = total
                .checked_add(track.samples.len())
                .ok_or(MediaError::DecodedProgramLimit)?;
            if total > sample_limit {
                return Err(MediaError::DecodedProgramLimit);
            }
        }
        Ok(Self { tracks })
    }

    pub fn primary_label(&self) -> &SourceLabel {
        &self.tracks[0].label
    }
}

#[derive(Debug, Clone)]
pub struct DeviceTrack {
    pub samples: Arc<[f32]>,
    pub channels: usize,
    pub frames: usize,
    pub regions: Vec<AuthoredRegion>,
    pub label: SourceLabel,
}

#[derive(Debug, Clone)]
pub struct DeviceProgram {
    pub tracks: Vec<DeviceTrack>,
    pub sample_rate_hz: u32,
    pub channels: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("media path is not a plain regular file: {0}")]
    UnsafeFile(PathBuf),
    #[error("media file size {actual} differs from the validated manifest size {expected}")]
    FileSizeMismatch { expected: u64, actual: u64 },
    #[error("media file exceeds the {MAX_ENCODED_BYTES}-byte decode limit")]
    EncodedLimit,
    #[error("unsupported media extension or codec declaration")]
    CodecMismatch,
    #[error("media decode failed: {0}")]
    Decode(String),
    #[error("decoded media sample rate {actual} differs from manifest value {expected}")]
    SampleRateMismatch { expected: u32, actual: u32 },
    #[error("decoded media channel count {actual} differs from manifest value {expected}")]
    ChannelMismatch { expected: u16, actual: u16 },
    #[error("decoded media bit depth {actual} differs from manifest value {expected}")]
    BitDepthMismatch { expected: u16, actual: u16 },
    #[error("only mono and stereo pack assets are playable; got {0} channels")]
    UnsupportedChannels(u16),
    #[error("decoded media contains a non-finite sample")]
    NonFiniteSample,
    #[error("decoded media is empty")]
    Empty,
    #[error("decoded media exceeds the remaining playback-program sample limit")]
    DecodedLimit,
    #[error("decoded playback program exceeds the {MAX_PROGRAM_SAMPLES}-sample aggregate limit")]
    DecodedProgramLimit,
    #[error("decoded duration {actual:.3}s differs from manifest duration {expected:.3}s")]
    DurationMismatch { expected: f64, actual: f64 },
    #[error("playback program must contain 1..={MAX_PROGRAM_TRACKS} tracks, got {0}")]
    InvalidProgramSize(usize),
    #[error("audio output has an invalid sample rate or channel count")]
    InvalidDeviceFormat,
    #[error("decoded track has an invalid rate, channel count, or interleaved length")]
    InvalidDecodedLayout,
    #[error("resampling failed: {0}")]
    Resample(String),
    #[error("device-rate playback PCM exceeds the aggregate device-program sample limit")]
    DevicePcmLimit,
    #[error("media SHA-256 differs from the fully revalidated manifest")]
    HashMismatch,
    #[error("decoded or resampled PCM is not channel-aligned")]
    UnalignedSamples,
    #[error("prepared tracks have no valid authored continuous-playback transition")]
    MissingContinuousTransition,
}

/// Decode a fully revalidated installed asset into bounded interleaved f32 PCM.
pub fn decode_track(expectation: &DecodeExpectation) -> Result<DecodedTrack, MediaError> {
    decode_track_with_limit(expectation, MAX_PROGRAM_SAMPLES)
}

/// Decodes one generated FLAC after its caller has confined the path to an
/// app-owned job output location. Generated drafts have no manifest yet, but
/// still receive the same regular-file, codec, finite-sample, and bounded-size
/// checks as installed media.
pub fn decode_generated_draft_flac(
    path: PathBuf,
    label: SourceLabel,
) -> Result<DecodedTrack, MediaError> {
    let metadata = fs::symlink_metadata(&path).map_err(|_| MediaError::UnsafeFile(path.clone()))?;
    if is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_ENCODED_BYTES
    {
        return Err(MediaError::UnsafeFile(path));
    }
    let file = File::open(&path).map_err(|error| MediaError::Decode(error.to_string()))?;
    let stream = MediaSourceStream::new(
        Box::new(
            file.try_clone()
                .map_err(|error| MediaError::Decode(error.to_string()))?,
        ),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(map_decode_error)?;
    if format.format_info().short_name != MediaCodec::Flac.format_name() {
        return Err(MediaError::CodecMismatch);
    }
    let (track_id, params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| MediaError::Decode("no default audio track".to_owned()))?;
        (
            track.id,
            track
                .codec_params
                .as_ref()
                .and_then(|params| params.audio())
                .ok_or_else(|| {
                    MediaError::Decode("audio track has no codec parameters".to_owned())
                })?
                .clone(),
        )
    };
    if !MediaCodec::Flac.accepts(params.codec) {
        return Err(MediaError::CodecMismatch);
    }
    let sample_rate = params
        .sample_rate
        .ok_or_else(|| MediaError::Decode("audio track has no sample rate".to_owned()))?;
    let channels = params
        .channels
        .as_ref()
        .ok_or_else(|| MediaError::Decode("audio track has no channels".to_owned()))?
        .count() as u16;
    if channels == 0 || channels > 2 {
        return Err(MediaError::UnsupportedChannels(channels));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(map_decode_error)?;
    let mut samples = Vec::new();
    while let Some(packet) = format.next_packet().map_err(map_decode_error)? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(map_decode_error)?;
        if decoded.spec().rate() != sample_rate
            || decoded.spec().channels().count() as u16 != channels
        {
            return Err(MediaError::Decode(
                "generated audio format changed while decoding".to_owned(),
            ));
        }
        let count = decoded.samples_interleaved();
        let next = samples
            .len()
            .checked_add(count)
            .filter(|length| *length <= MAX_PROGRAM_SAMPLES)
            .ok_or(MediaError::DecodedLimit)?;
        let start = samples.len();
        samples.resize(next, f32::MID);
        decoded.copy_to_slice_interleaved(&mut samples[start..]);
    }
    if samples.is_empty() {
        return Err(MediaError::Empty);
    }
    if !samples.len().is_multiple_of(usize::from(channels))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(MediaError::InvalidDecodedLayout);
    }
    // Keep the handle alive through decode and make a final size check to avoid
    // accepting a concurrently replaced output.
    if file
        .metadata()
        .map_err(|error| MediaError::Decode(error.to_string()))?
        .len()
        != metadata.len()
    {
        return Err(MediaError::FileSizeMismatch {
            expected: metadata.len(),
            actual: file
                .metadata()
                .map_err(|error| MediaError::Decode(error.to_string()))?
                .len(),
        });
    }
    Ok(DecodedTrack {
        sample_rate_hz: sample_rate,
        channels,
        samples: samples.into(),
        regions: vec![],
        label,
    })
}

pub fn decode_track_with_limit(
    expectation: &DecodeExpectation,
    sample_limit: usize,
) -> Result<DecodedTrack, MediaError> {
    let sample_limit = sample_limit.min(MAX_PROGRAM_SAMPLES);
    if expectation.channels == 0 || expectation.channels > 2 {
        return Err(MediaError::UnsupportedChannels(expectation.channels));
    }
    if expectation.bytes == 0 || expectation.bytes > MAX_ENCODED_BYTES || sample_limit == 0 {
        return Err(MediaError::EncodedLimit);
    }
    if expectation.sha256.len() != 64
        || !expectation
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(MediaError::HashMismatch);
    }
    if expectation
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case(expectation.codec.extension()))
    {
        return Err(MediaError::CodecMismatch);
    }
    let metadata = fs::symlink_metadata(&expectation.path)
        .map_err(|_| MediaError::UnsafeFile(expectation.path.clone()))?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(MediaError::UnsafeFile(expectation.path.clone()));
    }
    if metadata.len() != expectation.bytes {
        return Err(MediaError::FileSizeMismatch {
            expected: expectation.bytes,
            actual: metadata.len(),
        });
    }
    let mut file =
        File::open(&expectation.path).map_err(|error| MediaError::Decode(error.to_string()))?;
    let handle_metadata = file
        .metadata()
        .map_err(|error| MediaError::Decode(error.to_string()))?;
    if !handle_metadata.is_file() || handle_metadata.len() != expectation.bytes {
        return Err(MediaError::FileSizeMismatch {
            expected: expectation.bytes,
            actual: handle_metadata.len(),
        });
    }
    verify_open_file_hash(&mut file, expectation.bytes, &expectation.sha256)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| MediaError::Decode(error.to_string()))?;

    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(expectation.codec.extension());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(map_decode_error)?;
    if format.format_info().short_name != expectation.codec.format_name() {
        return Err(MediaError::CodecMismatch);
    }
    let (track_id, params, track_delay) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| MediaError::Decode("no default audio track".to_owned()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| MediaError::Decode("audio track has no codec parameters".to_owned()))?
            .clone();
        (track.id, params, track.delay)
    };
    if !expectation.codec.accepts(params.codec) {
        return Err(MediaError::CodecMismatch);
    }
    if let Some(rate) = params.sample_rate {
        check_rate(expectation.sample_rate_hz, rate)?;
    }
    if let Some(channels) = &params.channels {
        check_channels(expectation.channels, channels.count() as u16)?;
    }
    if let Some(expected) = expectation.bit_depth {
        match params.bits_per_sample {
            Some(actual) if u32::from(expected) == actual => {}
            Some(actual) => {
                return Err(MediaError::BitDepthMismatch {
                    expected,
                    actual: actual as u16,
                });
            }
            None => {
                return Err(MediaError::Decode(
                    "decoder did not report the manifest-declared bit depth".to_owned(),
                ));
            }
        }
    }
    let samples = if expectation.codec == MediaCodec::OggOpus {
        decode_ogg_opus(
            format.as_mut(),
            track_id,
            &params,
            track_delay,
            expectation.channels,
            sample_limit,
        )?
    } else {
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(map_decode_error)?;
        let mut samples = Vec::<f32>::new();
        while let Some(packet) = format.next_packet().map_err(map_decode_error)? {
            if packet.track_id != track_id {
                continue;
            }
            let decoded = decoder.decode(&packet).map_err(map_decode_error)?;
            let spec = decoded.spec();
            check_rate(expectation.sample_rate_hz, spec.rate())?;
            check_channels(expectation.channels, spec.channels().count() as u16)?;
            let packet_samples = decoded.samples_interleaved();
            if !packet_samples.is_multiple_of(usize::from(expectation.channels)) {
                return Err(MediaError::UnalignedSamples);
            }
            let next_len = samples
                .len()
                .checked_add(packet_samples)
                .filter(|length| *length <= sample_limit)
                .ok_or(MediaError::DecodedLimit)?;
            let start = samples.len();
            samples.resize(next_len, f32::MID);
            decoded.copy_to_slice_interleaved(&mut samples[start..]);
            if samples[start..].iter().any(|sample| !sample.is_finite()) {
                return Err(MediaError::NonFiniteSample);
            }
        }
        samples
    };
    if samples.is_empty() {
        return Err(MediaError::Empty);
    }
    if !samples
        .len()
        .is_multiple_of(usize::from(expectation.channels))
    {
        return Err(MediaError::UnalignedSamples);
    }
    let frames = samples.len() / usize::from(expectation.channels);
    let actual_duration = frames as f64 / f64::from(expectation.sample_rate_hz);
    let expected_duration = f64::from(expectation.duration_seconds);
    if (actual_duration - expected_duration).abs() > MAX_DURATION_MISMATCH_SECONDS {
        return Err(MediaError::DurationMismatch {
            expected: expected_duration,
            actual: actual_duration,
        });
    }
    Ok(DecodedTrack {
        sample_rate_hz: expectation.sample_rate_hz,
        channels: expectation.channels,
        samples: samples.into(),
        regions: expectation.regions.clone(),
        label: expectation.label.clone(),
    })
}

fn decode_ogg_opus(
    format: &mut dyn FormatReader,
    track_id: u32,
    params: &AudioCodecParameters,
    track_delay: Option<u32>,
    channels: u16,
    sample_limit: usize,
) -> Result<Vec<f32>, MediaError> {
    const OPUS_RATE: u32 = 48_000;
    const MAX_OPUS_PACKET_FRAMES: usize = 5_760;
    let header = params
        .extra_data
        .as_deref()
        .ok_or(MediaError::CodecMismatch)?;
    if header.len() != 19
        || &header[..8] != b"OpusHead"
        || header[8] > 15
        || u16::from(header[9]) != channels
        || header[18] != 0
    {
        return Err(MediaError::CodecMismatch);
    }
    let pre_skip = u16::from_le_bytes([header[10], header[11]]);
    if track_delay != Some(u32::from(pre_skip)) {
        return Err(MediaError::CodecMismatch);
    }
    let gain_q8_db = i16::from_le_bytes([header[16], header[17]]);
    let channel_mode = match channels {
        1 => OpusChannels::Mono,
        2 => OpusChannels::Stereo,
        _ => return Err(MediaError::UnsupportedChannels(channels)),
    };
    let mut decoder = OpusDecoder::new(OPUS_RATE, channel_mode)
        .map_err(|error| MediaError::Decode(error.to_string()))?;
    decoder
        .set_gain(i32::from(gain_q8_db))
        .map_err(|error| MediaError::Decode(error.to_string()))?;

    let channel_count = usize::from(channels);
    let mut packet_pcm = vec![0.0_f32; MAX_OPUS_PACKET_FRAMES * channel_count];
    let mut samples = Vec::new();
    // Symphonia 0.6 exposes OpusHead's pre-skip as Track::delay but its Ogg
    // packet mapper does not include it in packet.trim_start. Apply it once
    // here before the PCM is made available to authored loop regions.
    let mut remaining_pre_skip = usize::from(pre_skip);
    while let Some(packet) = format.next_packet().map_err(map_decode_error)? {
        if packet.track_id != track_id {
            continue;
        }
        if packet.data.is_empty() {
            return Err(MediaError::Decode("empty Ogg Opus packet".to_owned()));
        }
        let decoded_frames = decoder
            .decode_float(&packet.data, &mut packet_pcm, false)
            .map_err(|error| MediaError::Decode(error.to_string()))?;
        let block_frames =
            usize::try_from(packet.block_dur().get()).map_err(|_| MediaError::DecodedLimit)?;
        if decoded_frames != block_frames || decoded_frames > MAX_OPUS_PACKET_FRAMES {
            return Err(MediaError::Decode(
                "Ogg packet timing differs from the decoded Opus frame count".to_owned(),
            ));
        }
        let trim_start =
            usize::try_from(packet.trim_start.get()).map_err(|_| MediaError::DecodedLimit)?;
        let trim_end =
            usize::try_from(packet.trim_end.get()).map_err(|_| MediaError::DecodedLimit)?;
        let container_valid_frames = decoded_frames
            .checked_sub(trim_start)
            .and_then(|frames| frames.checked_sub(trim_end))
            .ok_or_else(|| MediaError::Decode("invalid Ogg Opus packet trimming".to_owned()))?;
        if u64::try_from(container_valid_frames).ok() != Some(packet.dur.get()) {
            return Err(MediaError::Decode(
                "Ogg packet duration differs from its trimmed Opus frame count".to_owned(),
            ));
        }
        let pre_skip_now = remaining_pre_skip.min(container_valid_frames);
        remaining_pre_skip -= pre_skip_now;
        let valid_frames = container_valid_frames - pre_skip_now;
        let valid_samples = valid_frames
            .checked_mul(channel_count)
            .ok_or(MediaError::DecodedLimit)?;
        let next_len = samples
            .len()
            .checked_add(valid_samples)
            .filter(|length| *length <= sample_limit)
            .ok_or(MediaError::DecodedLimit)?;
        let start_sample = trim_start
            .checked_add(pre_skip_now)
            .ok_or(MediaError::DecodedLimit)?
            .checked_mul(channel_count)
            .ok_or(MediaError::DecodedLimit)?;
        let end_sample = start_sample
            .checked_add(valid_samples)
            .ok_or(MediaError::DecodedLimit)?;
        let valid = packet_pcm
            .get(start_sample..end_sample)
            .ok_or(MediaError::UnalignedSamples)?;
        if valid.iter().any(|sample| !sample.is_finite()) {
            return Err(MediaError::NonFiniteSample);
        }
        samples.reserve(next_len - samples.len());
        samples.extend_from_slice(valid);
    }
    if remaining_pre_skip != 0 {
        return Err(MediaError::Decode(
            "Ogg Opus stream ended before its declared pre-skip".to_owned(),
        ));
    }
    Ok(samples)
}

fn verify_open_file_hash(
    file: &mut File,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<(), MediaError> {
    let mut hasher = Sha256::new();
    let mut remaining = expected_bytes;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let request = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| MediaError::EncodedLimit)?;
        let read = file
            .read(&mut buffer[..request])
            .map_err(|error| MediaError::Decode(error.to_string()))?;
        if read == 0 {
            return Err(MediaError::FileSizeMismatch {
                expected: expected_bytes,
                actual: expected_bytes - remaining,
            });
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .map_err(|error| MediaError::Decode(error.to_string()))?
        != 0
    {
        return Err(MediaError::EncodedLimit);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_sha256) {
        Ok(())
    } else {
        Err(MediaError::HashMismatch)
    }
}

pub fn adapt_program_for_device(
    program: &DecodedProgram,
    output_rate_hz: u32,
    output_channels: usize,
) -> Result<DeviceProgram, MediaError> {
    adapt_program_for_device_with_limit(
        program,
        output_rate_hz,
        output_channels,
        MAX_DEVICE_PROGRAM_SAMPLES,
    )
}

fn adapt_program_for_device_with_limit(
    program: &DecodedProgram,
    output_rate_hz: u32,
    output_channels: usize,
    program_sample_limit: usize,
) -> Result<DeviceProgram, MediaError> {
    if output_rate_hz == 0 || output_channels == 0 {
        return Err(MediaError::InvalidDeviceFormat);
    }
    let mut tracks = Vec::with_capacity(program.tracks.len());
    let mut remaining_samples = program_sample_limit;
    for track in &program.tracks {
        let adapted = adapt_track(track, output_rate_hz, output_channels, remaining_samples)?;
        remaining_samples = remaining_samples
            .checked_sub(adapted.samples.len())
            .ok_or(MediaError::DevicePcmLimit)?;
        tracks.push(adapted);
    }
    Ok(DeviceProgram {
        tracks,
        sample_rate_hz: output_rate_hz,
        channels: output_channels,
    })
}

fn adapt_track(
    track: &DecodedTrack,
    output_rate_hz: u32,
    output_channels: usize,
    sample_limit: usize,
) -> Result<DeviceTrack, MediaError> {
    let input_channels = usize::from(track.channels);
    if !matches!(track.channels, 1 | 2) {
        return Err(MediaError::UnsupportedChannels(track.channels));
    }
    if track.sample_rate_hz == 0
        || track.samples.is_empty()
        || !track.samples.len().is_multiple_of(input_channels)
    {
        return Err(MediaError::InvalidDecodedLayout);
    }
    let input_frames = track.samples.len() / input_channels;
    let predicted_frames = (input_frames as u128 * u128::from(output_rate_hz))
        .div_ceil(u128::from(track.sample_rate_hz));
    if predicted_frames
        .checked_mul(output_channels as u128)
        .is_none_or(|samples| samples > sample_limit as u128)
    {
        return Err(MediaError::DevicePcmLimit);
    }
    let resampled = if track.sample_rate_hz == output_rate_hz {
        track.samples.to_vec()
    } else {
        let input = InterleavedSlice::new(&track.samples, input_channels, input_frames)
            .map_err(|error| MediaError::Resample(error.to_string()))?;
        let mut resampler = Fft::<f32>::new(
            track.sample_rate_hz as usize,
            output_rate_hz as usize,
            RESAMPLER_CHUNK_FRAMES,
            input_channels,
            FixedSync::Input,
        )
        .map_err(|error| MediaError::Resample(error.to_string()))?;
        resampler
            .process_all(&input, input_frames, None)
            .map_err(|error| MediaError::Resample(error.to_string()))?
            .take_data()
    };
    if !resampled.len().is_multiple_of(input_channels) {
        return Err(MediaError::UnalignedSamples);
    }
    if resampled.iter().any(|sample| !sample.is_finite()) {
        return Err(MediaError::NonFiniteSample);
    }
    let resampled_frames = resampled.len() / input_channels;
    let device_samples = resampled_frames
        .checked_mul(output_channels)
        .filter(|samples| *samples <= sample_limit)
        .ok_or(MediaError::DevicePcmLimit)?;
    let mut mapped = Vec::with_capacity(device_samples);
    for frame in resampled.chunks_exact(input_channels) {
        match input_channels {
            1 => mapped.extend(std::iter::repeat_n(frame[0], output_channels)),
            2 => {
                for channel in 0..output_channels {
                    mapped.push(match channel {
                        0 => frame[0],
                        1 => frame[1],
                        _ => 0.5 * (frame[0] + frame[1]),
                    });
                }
            }
            _ => return Err(MediaError::UnsupportedChannels(track.channels)),
        }
    }
    if mapped.len() != device_samples || mapped.iter().any(|sample| !sample.is_finite()) {
        return Err(MediaError::NonFiniteSample);
    }
    Ok(DeviceTrack {
        frames: resampled_frames,
        samples: mapped.into(),
        channels: output_channels,
        regions: track.regions.clone(),
        label: track.label.clone(),
    })
}

fn check_rate(expected: u32, actual: u32) -> Result<(), MediaError> {
    if expected == actual {
        Ok(())
    } else {
        Err(MediaError::SampleRateMismatch { expected, actual })
    }
}

fn check_channels(expected: u16, actual: u16) -> Result<(), MediaError> {
    if expected == actual {
        Ok(())
    } else {
        Err(MediaError::ChannelMismatch { expected, actual })
    }
}

fn map_decode_error(error: SymphoniaError) -> MediaError {
    MediaError::Decode(error.to_string())
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn wav_bytes(sample_rate: u32, frames: u32) -> Vec<u8> {
        let data_len = frames * 2;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for index in 0..frames {
            let sample = ((index as f32 * 0.031).sin() * 8_000.0) as i16;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    fn expectation(path: PathBuf, bytes: &[u8]) -> DecodeExpectation {
        DecodeExpectation {
            path,
            codec: MediaCodec::Wav,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            sample_rate_hz: 8_000,
            channels: 1,
            bit_depth: Some(16),
            duration_seconds: 1.0,
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.2,
                end_seconds: 0.8,
            }],
            label: SourceLabel::test_fallback(),
        }
    }

    fn decoded(rate: u32, channels: u16, frames: usize) -> DecodedTrack {
        let samples = (0..frames * usize::from(channels))
            .map(|index| (index as f32 * 0.013).sin() * 0.25)
            .collect::<Vec<_>>();
        DecodedTrack {
            sample_rate_hz: rate,
            channels,
            samples: samples.into(),
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.01,
                end_seconds: 0.05,
            }],
            label: SourceLabel::test_fallback(),
        }
    }

    #[derive(Clone, Copy)]
    struct FixtureSpec {
        file_name: &'static str,
        bytes: &'static [u8],
        codec: MediaCodec,
        sample_rate_hz: u32,
        channels: u16,
        bit_depth: Option<u16>,
        duration_seconds: f32,
    }

    fn decode_fixture(directory: &tempfile::TempDir, fixture: FixtureSpec) -> DecodedTrack {
        let path = directory.path().join(fixture.file_name);
        File::create(&path)
            .unwrap()
            .write_all(fixture.bytes)
            .unwrap();
        decode_track(&DecodeExpectation {
            path,
            codec: fixture.codec,
            bytes: fixture.bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(fixture.bytes)),
            sample_rate_hz: fixture.sample_rate_hz,
            channels: fixture.channels,
            bit_depth: fixture.bit_depth,
            duration_seconds: fixture.duration_seconds,
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.02,
                end_seconds: fixture.duration_seconds - 0.02,
            }],
            label: SourceLabel::test_fallback(),
        })
        .unwrap()
    }

    #[test]
    fn resamples_once_and_maps_mono_to_device_layout() {
        let program = DecodedProgram::new(vec![decoded(44_100, 1, 4_410)]).unwrap();
        let output = adapt_program_for_device(&program, 48_000, 2).unwrap();
        assert_eq!(output.channels, 2);
        assert_eq!(output.tracks[0].samples.len(), output.tracks[0].frames * 2);
        assert!(output.tracks[0]
            .samples
            .chunks_exact(2)
            .all(|frame| frame[0] == frame[1]));
        assert!((output.tracks[0].frames as isize - 4_800).abs() <= 2);
    }

    #[test]
    fn stereo_extra_device_channels_use_bounded_downmix() {
        let program = DecodedProgram::new(vec![decoded(48_000, 2, 480)]).unwrap();
        let output = adapt_program_for_device(&program, 48_000, 4).unwrap();
        for frame in output.tracks[0].samples.chunks_exact(4) {
            let centre = 0.5 * (frame[0] + frame[1]);
            assert_eq!(frame[2], centre);
            assert_eq!(frame[3], centre);
        }
    }

    #[test]
    fn invalid_program_and_multichannel_inputs_are_rejected() {
        assert!(matches!(
            DecodedProgram::new(Vec::new()),
            Err(MediaError::InvalidProgramSize(0))
        ));
        assert!(matches!(
            DecodedProgram::new(vec![decoded(48_000, 3, 100)]),
            Err(MediaError::InvalidDecodedLayout)
        ));
    }

    #[test]
    fn decoded_program_accepts_eight_loop_tracks_and_rejects_nine_within_budget() {
        let eight = (0..8).map(|_| decoded(48_000, 1, 100)).collect::<Vec<_>>();
        let program = DecodedProgram::new(eight).unwrap();
        assert_eq!(program.tracks.len(), MAX_PROGRAM_TRACKS);

        // Adapting the bounded queue stays within the device sample budget and
        // preserves every track at the device channel layout.
        let device = adapt_program_for_device(&program, 48_000, 2).unwrap();
        assert_eq!(device.tracks.len(), MAX_PROGRAM_TRACKS);
        assert_eq!(device.channels, 2);
        for track in &device.tracks {
            assert_eq!(track.samples.len(), track.frames * 2);
        }

        let nine = (0..9).map(|_| decoded(48_000, 1, 100)).collect::<Vec<_>>();
        assert!(matches!(
            DecodedProgram::new(nine),
            Err(MediaError::InvalidProgramSize(9))
        ));
    }

    #[test]
    fn decoder_checks_validated_file_and_technical_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.wav");
        let bytes = wav_bytes(8_000, 8_000);
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let expected = expectation(path, &bytes);
        let decoded = decode_track(&expected).unwrap();
        assert_eq!(decoded.sample_rate_hz, 8_000);
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples.len(), 8_000);
    }

    #[test]
    fn corrupt_size_and_rate_mismatches_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.wav");
        let bytes = wav_bytes(8_000, 8_000);
        File::create(&path).unwrap().write_all(&bytes).unwrap();

        let mut wrong_size = expectation(path.clone(), &bytes);
        wrong_size.bytes += 1;
        assert!(matches!(
            decode_track(&wrong_size),
            Err(MediaError::FileSizeMismatch { .. })
        ));
        wrong_size.bytes = bytes.len() as u64;
        wrong_size.sample_rate_hz = 44_100;
        assert!(matches!(
            decode_track(&wrong_size),
            Err(MediaError::SampleRateMismatch { .. })
        ));

        let corrupt_path = directory.path().join("corrupt.wav");
        File::create(&corrupt_path)
            .unwrap()
            .write_all(&[0u8; 64])
            .unwrap();
        assert!(matches!(
            decode_track(&expectation(corrupt_path, &[0u8; 64])),
            Err(MediaError::Decode(_))
        ));
    }

    #[test]
    fn sha256_must_be_canonical_lowercase_and_rejects_substitution() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.wav");
        let bytes = wav_bytes(8_000, 8_000);
        File::create(&path).unwrap().write_all(&bytes).unwrap();

        // Published manifests use one lowercase canonical spelling.
        let mut upper = expectation(path.clone(), &bytes);
        upper.sha256 = upper.sha256.to_ascii_uppercase();
        assert!(matches!(
            decode_track(&upper),
            Err(MediaError::HashMismatch)
        ));

        // A different file with the same byte count must not pass the hash gate.
        let substituted_path = directory.path().join("substituted.wav");
        let mut different = bytes.clone();
        different[44] ^= 0xff;
        File::create(&substituted_path)
            .unwrap()
            .write_all(&different)
            .unwrap();
        let same_size = expectation(substituted_path, &bytes);
        assert_eq!(same_size.bytes, different.len() as u64);
        assert!(matches!(
            decode_track(&same_size),
            Err(MediaError::HashMismatch)
        ));

        // Non-hex characters and wrong length are rejected before opening.
        let mut bad_hex = expectation(path, &bytes);
        bad_hex.sha256 = bad_hex.sha256.replace('a', "g");
        assert!(matches!(
            decode_track(&bad_hex),
            Err(MediaError::HashMismatch)
        ));
    }

    #[test]
    fn advertised_decoders_read_real_committed_fixtures() {
        let directory = tempfile::tempdir().unwrap();
        let cases = [
            FixtureSpec {
                file_name: "mono.wav",
                bytes: include_bytes!("../tests/fixtures/wav_pcm16_mono_44100.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 44_100,
                channels: 1,
                bit_depth: Some(16),
                duration_seconds: 1.0,
            },
            FixtureSpec {
                file_name: "stereo24.wav",
                bytes: include_bytes!("../tests/fixtures/wav_pcm24_stereo_48000.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 48_000,
                channels: 2,
                bit_depth: Some(24),
                duration_seconds: 1.0,
            },
            FixtureSpec {
                file_name: "stereo-float.wav",
                bytes: include_bytes!("../tests/fixtures/wav_f32_stereo_96000.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 96_000,
                channels: 2,
                bit_depth: Some(32),
                duration_seconds: 1.0,
            },
            FixtureSpec {
                file_name: "mono.flac",
                bytes: include_bytes!("../tests/fixtures/flac_mono_44100.flac"),
                codec: MediaCodec::Flac,
                sample_rate_hz: 44_100,
                channels: 1,
                bit_depth: Some(16),
                duration_seconds: 1.0,
            },
            FixtureSpec {
                file_name: "stereo.mp3",
                bytes: include_bytes!("../tests/fixtures/mp3_stereo_48000.mp3"),
                codec: MediaCodec::Mp3,
                sample_rate_hz: 48_000,
                channels: 2,
                bit_depth: None,
                duration_seconds: 1.0,
            },
            FixtureSpec {
                file_name: "stereo.opus",
                bytes: include_bytes!("../tests/fixtures/ogg_opus_stereo_48000.opus"),
                codec: MediaCodec::OggOpus,
                sample_rate_hz: 48_000,
                channels: 2,
                bit_depth: None,
                duration_seconds: 1.0,
            },
        ];
        for fixture in cases {
            let decoded = decode_fixture(&directory, fixture);
            assert_eq!(decoded.sample_rate_hz, fixture.sample_rate_hz);
            assert_eq!(decoded.channels, fixture.channels);
            assert!(decoded.samples.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn wav_contract_rejects_non_pcm_and_wrong_declared_depth() {
        use symphonia::core::codecs::audio::well_known::{
            CODEC_ID_ADPCM_MS, CODEC_ID_PCM_ALAW, CODEC_ID_PCM_MULAW,
        };
        assert!(!MediaCodec::Wav.accepts(CODEC_ID_ADPCM_MS));
        assert!(!MediaCodec::Wav.accepts(CODEC_ID_PCM_ALAW));
        assert!(!MediaCodec::Wav.accepts(CODEC_ID_PCM_MULAW));
        assert!(MediaCodec::Wav.accepts(CODEC_ID_PCM_S16LE));
        assert!(MediaCodec::Wav.accepts(CODEC_ID_PCM_F32LE));
    }

    #[test]
    fn fixture_matrix_resamples_44100_48000_and_96000_without_channel_swap() {
        let directory = tempfile::tempdir().unwrap();
        let mono = decode_fixture(
            &directory,
            FixtureSpec {
                file_name: "mono.wav",
                bytes: include_bytes!("../tests/fixtures/wav_pcm16_mono_44100.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 44_100,
                channels: 1,
                bit_depth: Some(16),
                duration_seconds: 1.0,
            },
        );
        let stereo = decode_fixture(
            &directory,
            FixtureSpec {
                file_name: "stereo.wav",
                bytes: include_bytes!("../tests/fixtures/wav_pcm24_stereo_48000.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 48_000,
                channels: 2,
                bit_depth: Some(24),
                duration_seconds: 1.0,
            },
        );
        let high_rate = decode_fixture(
            &directory,
            FixtureSpec {
                file_name: "high-rate.wav",
                bytes: include_bytes!("../tests/fixtures/wav_f32_stereo_96000.wav"),
                codec: MediaCodec::Wav,
                sample_rate_hz: 96_000,
                channels: 2,
                bit_depth: Some(32),
                duration_seconds: 1.0,
            },
        );
        let output =
            adapt_program_for_device(&DecodedProgram::new(vec![mono, stereo]).unwrap(), 48_000, 2)
                .unwrap();
        assert!(output.tracks[0]
            .samples
            .chunks_exact(2)
            .all(|frame| frame[0] == frame[1]));
        let high_output =
            adapt_program_for_device(&DecodedProgram::new(vec![high_rate]).unwrap(), 48_000, 2)
                .unwrap();
        assert!((high_output.tracks[0].frames as isize - 48_000).abs() <= 2);
    }

    #[test]
    fn truncation_nonfinite_and_aggregate_bounds_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../tests/fixtures/wav_pcm16_mono_44100.wav");
        let limited_path = directory.path().join("limited.wav");
        File::create(&limited_path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        let limited = DecodeExpectation {
            path: limited_path,
            codec: MediaCodec::Wav,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            sample_rate_hz: 44_100,
            channels: 1,
            bit_depth: Some(16),
            duration_seconds: 1.0,
            regions: Vec::new(),
            label: SourceLabel::test_fallback(),
        };
        assert!(matches!(
            decode_track_with_limit(&limited, 10),
            Err(MediaError::DecodedLimit)
        ));

        let empty_bytes = wav_bytes(8_000, 0);
        let empty_path = directory.path().join("empty.wav");
        File::create(&empty_path)
            .unwrap()
            .write_all(&empty_bytes)
            .unwrap();
        assert!(matches!(
            decode_track(&expectation(empty_path, &empty_bytes)),
            Err(MediaError::Empty | MediaError::Decode(_))
        ));

        let path = directory.path().join("truncated.wav");
        let truncated = &bytes[..bytes.len() / 2];
        File::create(&path).unwrap().write_all(truncated).unwrap();
        let truncated_expectation = DecodeExpectation {
            path,
            codec: MediaCodec::Wav,
            bytes: truncated.len() as u64,
            sha256: format!("{:x}", Sha256::digest(truncated)),
            sample_rate_hz: 44_100,
            channels: 1,
            bit_depth: Some(16),
            duration_seconds: 1.0,
            regions: Vec::new(),
            label: SourceLabel::test_fallback(),
        };
        assert!(matches!(
            decode_track(&truncated_expectation),
            Err(MediaError::Decode(_))
        ));

        let mut float_bytes = include_bytes!("../tests/fixtures/wav_f32_stereo_96000.wav").to_vec();
        let data = float_bytes
            .windows(4)
            .position(|window| window == b"data")
            .unwrap();
        float_bytes[data + 8..data + 12].copy_from_slice(&f32::NAN.to_le_bytes());
        let float_path = directory.path().join("nonfinite.wav");
        File::create(&float_path)
            .unwrap()
            .write_all(&float_bytes)
            .unwrap();
        assert!(matches!(
            decode_track(&DecodeExpectation {
                path: float_path,
                codec: MediaCodec::Wav,
                bytes: float_bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&float_bytes)),
                sample_rate_hz: 96_000,
                channels: 2,
                bit_depth: Some(32),
                duration_seconds: 1.0,
                regions: Vec::new(),
                label: SourceLabel::test_fallback(),
            }),
            Err(MediaError::NonFiniteSample)
        ));

        assert!(matches!(
            DecodedProgram::with_sample_limit(
                vec![decoded(48_000, 1, 8), decoded(48_000, 1, 8)],
                15,
            ),
            Err(MediaError::DecodedProgramLimit)
        ));
        let program =
            DecodedProgram::new(vec![decoded(48_000, 1, 8), decoded(48_000, 1, 8)]).unwrap();
        assert!(matches!(
            adapt_program_for_device_with_limit(&program, 48_000, 2, 31),
            Err(MediaError::DevicePcmLimit)
        ));
    }
}
