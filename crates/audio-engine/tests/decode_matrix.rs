//! Deterministic decoder/matrix coverage for every advertised codec and layout.
//!
//! These tests use the committed binary fixtures in `tests/fixtures/`. They do
//! not require FFmpeg at runtime; FFmpeg was used once to generate the assets
//! (see `tests/fixtures/provenance.md`).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use audio_engine::{
    adapt_program_for_device, decode_track, AuthoredRegion, AuthoredRegionKind, DecodeExpectation,
    DecodedProgram, DecodedTrack, MediaCodec, MediaError, SourceLabel,
};
use sha2::Digest;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_path(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    codec: MediaCodec,
    sample_rate_hz: u32,
    channels: u16,
    bit_depth: Option<u16>,
    expected_frames: usize,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "wav_pcm16_mono_44100.wav",
        codec: MediaCodec::Wav,
        sample_rate_hz: 44_100,
        channels: 1,
        bit_depth: Some(16),
        expected_frames: 44_100,
    },
    Fixture {
        name: "wav_pcm24_stereo_48000.wav",
        codec: MediaCodec::Wav,
        sample_rate_hz: 48_000,
        channels: 2,
        bit_depth: Some(24),
        expected_frames: 48_000,
    },
    Fixture {
        name: "wav_f32_stereo_96000.wav",
        codec: MediaCodec::Wav,
        sample_rate_hz: 96_000,
        channels: 2,
        bit_depth: Some(32),
        expected_frames: 96_000,
    },
    Fixture {
        name: "flac_mono_44100.flac",
        codec: MediaCodec::Flac,
        sample_rate_hz: 44_100,
        channels: 1,
        bit_depth: Some(16),
        expected_frames: 44_100,
    },
    Fixture {
        name: "flac_stereo_48000.flac",
        codec: MediaCodec::Flac,
        sample_rate_hz: 48_000,
        channels: 2,
        bit_depth: Some(16),
        expected_frames: 48_000,
    },
    Fixture {
        name: "mp3_mono_44100.mp3",
        codec: MediaCodec::Mp3,
        sample_rate_hz: 44_100,
        channels: 1,
        bit_depth: None,
        expected_frames: 44_100,
    },
    Fixture {
        name: "mp3_stereo_48000.mp3",
        codec: MediaCodec::Mp3,
        sample_rate_hz: 48_000,
        channels: 2,
        bit_depth: None,
        expected_frames: 48_000,
    },
    Fixture {
        name: "ogg_opus_stereo_48000.opus",
        codec: MediaCodec::OggOpus,
        sample_rate_hz: 48_000,
        channels: 2,
        bit_depth: None,
        expected_frames: 48_000,
    },
];

fn label() -> SourceLabel {
    SourceLabel::test_fallback()
}

fn expectation(fixture: Fixture) -> DecodeExpectation {
    let path = fixture_path(fixture.name);
    let bytes = fs::metadata(&path).unwrap().len();
    let sha = hex_lower(&fs::read(&path).unwrap());
    DecodeExpectation {
        path,
        codec: fixture.codec,
        bytes,
        sha256: sha,
        sample_rate_hz: fixture.sample_rate_hz,
        channels: fixture.channels,
        bit_depth: fixture.bit_depth,
        duration_seconds: 1.0,
        regions: vec![AuthoredRegion {
            kind: AuthoredRegionKind::Loop,
            start_seconds: 0.1,
            end_seconds: 0.9,
        }],
        label: label(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn decode_matrix_decodes_every_advertised_codec_rate_channel_and_bit_depth() {
    for fixture in FIXTURES {
        let decoded = decode_track(&expectation(*fixture))
            .unwrap_or_else(|error| panic!("{} failed to decode: {error}", fixture.name));
        assert_eq!(
            decoded.sample_rate_hz, fixture.sample_rate_hz,
            "{}",
            fixture.name
        );
        assert_eq!(decoded.channels, fixture.channels, "{}", fixture.name);
        assert_eq!(
            decoded.samples.len(),
            fixture.expected_frames * usize::from(fixture.channels),
            "{}",
            fixture.name
        );
        assert!(
            decoded
                .samples
                .len()
                .is_multiple_of(usize::from(fixture.channels)),
            "{} not channel-aligned",
            fixture.name
        );
        assert!(
            decoded.samples.iter().all(|sample| sample.is_finite()),
            "{} produced a non-finite sample",
            fixture.name
        );

        // Each decoded clip resamples once into the device layout and stays
        // finite and channel-aligned.
        let program = DecodedProgram::new(vec![decoded]).unwrap();
        let device = adapt_program_for_device(&program, 48_000, 2)
            .unwrap_or_else(|error| panic!("{} failed to adapt: {error}", fixture.name));
        assert_eq!(device.channels, 2, "{}", fixture.name);
        for track in &device.tracks {
            assert!(track.samples.len().is_multiple_of(2), "{}", fixture.name);
            assert!(
                track.samples.iter().all(|sample| sample.is_finite()),
                "{} resampled to a non-finite sample",
                fixture.name
            );
        }
    }
}

#[test]
fn noncanonical_hash_case_is_rejected() {
    let fixture = FIXTURES[0];
    let mut upper = expectation(fixture);
    upper.sha256 = upper.sha256.to_ascii_uppercase();
    assert!(matches!(
        decode_track(&upper),
        Err(MediaError::HashMismatch)
    ));
    let mut mixed = expectation(fixture);
    mixed.sha256 = mixed.sha256.to_ascii_uppercase();
    mixed.sha256[..32].make_ascii_lowercase();
    assert!(matches!(
        decode_track(&mixed),
        Err(MediaError::HashMismatch)
    ));
}

#[test]
fn zero_wav_decodes_to_exact_silence() {
    let path = fixture_path("wav_zero_mono_44100.wav");
    let bytes = fs::metadata(&path).unwrap().len();
    let sha256 = hex_lower(&fs::read(&path).unwrap());
    let exp = DecodeExpectation {
        path,
        codec: MediaCodec::Wav,
        bytes,
        sha256,
        sample_rate_hz: 44_100,
        channels: 1,
        bit_depth: Some(16),
        duration_seconds: 1.0,
        regions: vec![AuthoredRegion {
            kind: AuthoredRegionKind::Loop,
            start_seconds: 0.1,
            end_seconds: 0.9,
        }],
        label: label(),
    };
    let decoded = decode_track(&exp).unwrap();
    assert_eq!(decoded.samples.len(), 44_100);
    assert!(decoded.samples.iter().all(|sample| *sample == 0.0));
}

#[test]
fn truncation_size_mismatch_and_decode_failure_fail_closed() {
    let fixture = FIXTURES[0];
    let original = fs::read(fixture_path(fixture.name)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let truncated_path = temp.path().join("truncated.wav");
    let truncated = &original[..original.len() / 2];
    fs::write(&truncated_path, truncated).unwrap();

    // Declaring the original byte count against a shorter file fails on size.
    let mut size_mismatch = expectation(fixture);
    size_mismatch.path = truncated_path.clone();
    assert!(matches!(
        decode_track(&size_mismatch),
        Err(MediaError::FileSizeMismatch { .. })
    ));

    // Declaring the truncated byte count exposes a corrupt/short stream.
    let mut short = expectation(fixture);
    short.path = truncated_path;
    short.bytes = truncated.len() as u64;
    short.sha256 = hex_lower(truncated);
    let result = decode_track(&short);
    assert!(
        matches!(
            result,
            Err(MediaError::Decode(_))
                | Err(MediaError::UnalignedSamples)
                | Err(MediaError::DurationMismatch { .. })
        ),
        "truncated decode should fail closed, got {result:?}"
    );
}

#[test]
fn same_byte_count_substitution_is_rejected_by_hash() {
    let fixture = FIXTURES[0];
    let original = fs::read(fixture_path(fixture.name)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let substituted_path = temp.path().join("substituted.wav");
    let mut different = original.clone();
    different[44] ^= 0xff;
    fs::write(&substituted_path, &different).unwrap();

    let mut same_size = expectation(fixture);
    same_size.path = substituted_path;
    same_size.bytes = different.len() as u64;
    same_size.sha256 = hex_lower(&original);
    assert!(matches!(
        decode_track(&same_size),
        Err(MediaError::HashMismatch)
    ));
}

#[test]
fn metadata_mismatches_fail_closed() {
    let fixture = FIXTURES[0];

    let mut wrong_rate = expectation(fixture);
    wrong_rate.sample_rate_hz = 48_000;
    assert!(matches!(
        decode_track(&wrong_rate),
        Err(MediaError::SampleRateMismatch { .. })
    ));

    let mut wrong_channels = expectation(fixture);
    wrong_channels.channels = 2;
    assert!(matches!(
        decode_track(&wrong_channels),
        Err(MediaError::ChannelMismatch { .. })
    ));

    let mut wrong_depth = expectation(fixture);
    wrong_depth.bit_depth = Some(24);
    assert!(matches!(
        decode_track(&wrong_depth),
        Err(MediaError::BitDepthMismatch { .. })
    ));

    // An MP3 asset has no PCM bit depth; declaring one fails closed.
    let mp3 = FIXTURES[5];
    let mut declared_depth = expectation(mp3);
    declared_depth.bit_depth = Some(16);
    assert!(matches!(
        decode_track(&declared_depth),
        Err(MediaError::Decode(_))
    ));
}

#[test]
fn ogg_opus_rejects_wrong_extension_container_codec_metadata_duration_and_hash() {
    let opus = *FIXTURES.last().unwrap();
    let original = fs::read(fixture_path(opus.name)).unwrap();
    let temp = tempfile::tempdir().unwrap();

    let wrong_extension_path = temp.path().join("track.ogg");
    fs::write(&wrong_extension_path, &original).unwrap();
    let mut wrong_extension = expectation(opus);
    wrong_extension.path = wrong_extension_path;
    assert!(matches!(
        decode_track(&wrong_extension),
        Err(MediaError::CodecMismatch)
    ));

    let fake_opus_path = temp.path().join("not-opus.opus");
    let wav = fs::read(fixture_path(FIXTURES[0].name)).unwrap();
    fs::write(&fake_opus_path, &wav).unwrap();
    let mut wrong_container = expectation(opus);
    wrong_container.path = fake_opus_path;
    wrong_container.bytes = wav.len() as u64;
    wrong_container.sha256 = hex_lower(&wav);
    assert!(matches!(
        decode_track(&wrong_container),
        Err(MediaError::CodecMismatch)
    ));

    let mut wrong_codec = expectation(opus);
    wrong_codec.codec = MediaCodec::Flac;
    assert!(matches!(
        decode_track(&wrong_codec),
        Err(MediaError::CodecMismatch)
    ));

    let mut wrong_rate = expectation(opus);
    wrong_rate.sample_rate_hz = 44_100;
    assert!(matches!(
        decode_track(&wrong_rate),
        Err(MediaError::SampleRateMismatch { .. })
    ));

    let mut wrong_channels = expectation(opus);
    wrong_channels.channels = 1;
    assert!(matches!(
        decode_track(&wrong_channels),
        Err(MediaError::ChannelMismatch { .. })
    ));

    let mut wrong_duration = expectation(opus);
    wrong_duration.duration_seconds = 0.5;
    assert!(matches!(
        decode_track(&wrong_duration),
        Err(MediaError::DurationMismatch { .. })
    ));

    let mut wrong_hash = expectation(opus);
    wrong_hash.sha256 = "0".repeat(64);
    assert!(matches!(
        decode_track(&wrong_hash),
        Err(MediaError::HashMismatch)
    ));
}

#[test]
fn nonfinite_decoded_input_is_rejected_before_the_callback() {
    let track = DecodedTrack {
        sample_rate_hz: 48_000,
        channels: 1,
        samples: vec![0.5, f32::NAN, 0.5].into(),
        regions: vec![AuthoredRegion {
            kind: AuthoredRegionKind::Loop,
            start_seconds: 0.0,
            end_seconds: 0.00002,
        }],
        label: label(),
    };
    assert!(matches!(
        DecodedProgram::new(vec![track]),
        Err(MediaError::InvalidDecodedLayout)
    ));
}

#[test]
fn corrupt_wav_header_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let corrupt_path = temp.path().join("corrupt.wav");
    let mut file = fs::File::create(&corrupt_path).unwrap();
    file.write_all(&[0u8; 64]).unwrap();
    let exp = DecodeExpectation {
        path: corrupt_path,
        codec: MediaCodec::Wav,
        bytes: 64,
        sha256: hex_lower(&[0u8; 64]),
        sample_rate_hz: 44_100,
        channels: 1,
        bit_depth: Some(16),
        duration_seconds: 1.0,
        regions: vec![AuthoredRegion {
            kind: AuthoredRegionKind::Loop,
            start_seconds: 0.1,
            end_seconds: 0.9,
        }],
        label: label(),
    };
    assert!(matches!(decode_track(&exp), Err(MediaError::Decode(_))));
}
