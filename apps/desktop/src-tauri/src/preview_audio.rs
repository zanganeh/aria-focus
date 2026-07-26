use audio_engine::{
    decode_generated_draft_flac, decode_track_with_limit, AudioError, AudioFacade, AudioIntensity,
    AuthoredRegion, AuthoredRegionKind, DecodeExpectation, MediaCodec, PlaybackSource,
    PlaybackState, SourceLabel, MAX_PROGRAM_SAMPLES,
};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DraftPreviewState {
    Stopped,
    Playing,
    Paused,
}

pub(crate) struct CloudPreviewSource {
    pub(crate) path: PathBuf,
    pub(crate) codec: MediaCodec,
    pub(crate) sha256: String,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: u16,
    pub(crate) bit_depth: Option<u16>,
    pub(crate) duration_seconds: f32,
    pub(crate) item_id: String,
    pub(crate) title: String,
}

pub(crate) fn prepare_draft_preview(
    path: PathBuf,
    job_id: &str,
) -> Result<audio_engine::DecodedProgram, AudioError> {
    let track = decode_generated_draft_flac(
        path,
        SourceLabel {
            pack_id: "music-studio-draft".to_owned(),
            pack_title: "Music Studio draft — not saved or published".to_owned(),
            item_id: job_id.to_owned(),
            item_title: "Generated draft preview".to_owned(),
            variant_id: "generated-flac-v1".to_owned(),
        },
    )
    .map_err(|error| AudioError::Media(error.to_string()))?;
    audio_engine::DecodedProgram::new(vec![track])
        .map_err(|error| AudioError::Media(error.to_string()))
}

pub(crate) fn prepare_cloud_preview(
    source: CloudPreviewSource,
) -> Result<audio_engine::DecodedProgram, AudioError> {
    let bytes = fs::metadata(&source.path)
        .map_err(|error| AudioError::Media(error.to_string()))?
        .len();
    let track = decode_track_with_limit(
        &DecodeExpectation {
            path: source.path,
            codec: source.codec,
            bytes,
            sha256: source.sha256,
            sample_rate_hz: source.sample_rate_hz,
            channels: source.channels,
            bit_depth: source.bit_depth,
            duration_seconds: source.duration_seconds,
            regions: vec![AuthoredRegion {
                kind: AuthoredRegionKind::Loop,
                start_seconds: 0.0,
                end_seconds: source.duration_seconds,
            }],
            label: SourceLabel {
                pack_id: "cloud-generation-preview".to_owned(),
                pack_title: "Cloud generation preview — not saved".to_owned(),
                item_id: source.item_id.clone(),
                item_title: source.title,
                variant_id: format!("cloud-{}", source.codec.storage_name()),
            },
        },
        MAX_PROGRAM_SAMPLES,
    )
    .map_err(|error| AudioError::Media(error.to_string()))?;
    audio_engine::DecodedProgram::new(vec![track])
        .map_err(|error| AudioError::Media(error.to_string()))
}

/// Audio-only transport for generated drafts. It intentionally has no domain
/// session, preferences repository, history store, or persistence dependency.
pub(crate) struct PreviewAudioCoordinator<A: AudioFacade> {
    audio: A,
    job_id: Option<String>,
}

impl<A: AudioFacade> PreviewAudioCoordinator<A> {
    pub(crate) fn new(audio: A) -> Self {
        Self {
            audio,
            job_id: None,
        }
    }

    pub(crate) fn state(&self) -> DraftPreviewState {
        match self.audio.state() {
            PlaybackState::Stopped => DraftPreviewState::Stopped,
            PlaybackState::Playing => DraftPreviewState::Playing,
            PlaybackState::Paused => DraftPreviewState::Paused,
        }
    }
    pub(crate) fn current_job_id(&self) -> Option<&str> {
        self.job_id.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn start(
        &mut self,
        path: PathBuf,
        job_id: &str,
        master_volume: u8,
    ) -> Result<(), AudioError> {
        let program = prepare_draft_preview(path, job_id)?;
        self.start_prepared(program, job_id, master_volume)
    }

    pub(crate) fn start_prepared(
        &mut self,
        program: audio_engine::DecodedProgram,
        job_id: &str,
        master_volume: u8,
    ) -> Result<(), AudioError> {
        self.audio.set_master_volume(master_volume)?;
        self.audio
            .start_with_source(PlaybackSource::Draft(program), AudioIntensity::Off)?;
        self.job_id = Some(job_id.to_owned());
        Ok(())
    }

    pub(crate) fn pause(&mut self) -> Result<(), AudioError> {
        self.audio.pause()
    }
    pub(crate) fn resume(&mut self) -> Result<(), AudioError> {
        self.audio.resume()
    }
    pub(crate) fn stop(&mut self) -> Result<(), AudioError> {
        if self.state() == DraftPreviewState::Stopped {
            self.job_id = None;
            return Ok(());
        }
        self.audio.stop()?;
        self.job_id = None;
        Ok(())
    }
    pub(crate) fn set_master_volume(&mut self, percent: u8) -> Result<(), AudioError> {
        self.audio.set_master_volume(percent)
    }
}

/// Focus transport owns the audio output whenever a focus session begins.
/// Keeping this operation at the preview boundary makes it impossible for a
/// focus-start command to leave a draft stream active.
pub(crate) fn stop_for_focus_start<A: AudioFacade>(
    preview: &mut PreviewAudioCoordinator<A>,
) -> Result<(), AudioError> {
    preview.stop()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAudio {
        state: PlaybackState,
        fail_start: bool,
        volume: u8,
    }
    impl Default for MockAudio {
        fn default() -> Self {
            Self {
                state: PlaybackState::Stopped,
                fail_start: false,
                volume: 70,
            }
        }
    }
    impl AudioFacade for MockAudio {
        fn start(&mut self, _: AudioIntensity) -> Result<(), AudioError> {
            Ok(())
        }
        fn start_with_source(
            &mut self,
            _: PlaybackSource,
            _: AudioIntensity,
        ) -> Result<(), AudioError> {
            if self.fail_start {
                return Err(AudioError::InjectedFailure);
            }
            self.state = PlaybackState::Playing;
            Ok(())
        }
        fn pause(&mut self) -> Result<(), AudioError> {
            self.state = PlaybackState::Paused;
            Ok(())
        }
        fn resume(&mut self) -> Result<(), AudioError> {
            self.state = PlaybackState::Playing;
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            self.state = PlaybackState::Stopped;
            Ok(())
        }
        fn set_intensity(&mut self, _: AudioIntensity) -> Result<(), AudioError> {
            Ok(())
        }
        fn set_master_volume(&mut self, volume: u8) -> Result<(), AudioError> {
            self.volume = volume;
            Ok(())
        }
        fn master_volume(&self) -> u8 {
            self.volume
        }
        fn state(&self) -> PlaybackState {
            self.state
        }
        fn intensity(&self) -> AudioIntensity {
            AudioIntensity::Off
        }
    }

    fn valid_draft() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("job_preview_fixture.flac");
        std::fs::write(
            &path,
            include_bytes!("../../../../crates/audio-engine/tests/fixtures/flac_mono_44100.flac"),
        )
        .unwrap();
        (temp, path)
    }

    #[test]
    fn pause_resume_and_stop_have_exact_preview_states() {
        let (_temp, path) = valid_draft();
        let mut preview = PreviewAudioCoordinator::new(MockAudio::default());
        preview.start(path, "job_preview_fixture", 70).unwrap();
        assert_eq!(preview.state(), DraftPreviewState::Playing);
        preview.pause().unwrap();
        assert_eq!(preview.state(), DraftPreviewState::Paused);
        preview.resume().unwrap();
        assert_eq!(preview.state(), DraftPreviewState::Playing);
        preview.stop().unwrap();
        assert_eq!(preview.state(), DraftPreviewState::Stopped);
    }

    #[test]
    fn missing_or_corrupt_drafts_never_enter_preview_state() {
        let mut preview = PreviewAudioCoordinator::new(MockAudio::default());
        assert!(preview
            .start(PathBuf::from("missing.flac"), "job_missing", 70)
            .is_err());
        assert_eq!(preview.state(), DraftPreviewState::Stopped);
        let temp = tempfile::tempdir().unwrap();
        let corrupt = temp.path().join("corrupt.flac");
        std::fs::write(&corrupt, b"not a flac").unwrap();
        assert!(preview.start(corrupt, "job_corrupt", 70).is_err());
        assert_eq!(preview.state(), DraftPreviewState::Stopped);
    }

    #[test]
    fn audio_start_failure_leaves_no_preview_state() {
        let (_temp, path) = valid_draft();
        let mut preview = PreviewAudioCoordinator::new(MockAudio {
            fail_start: true,
            ..Default::default()
        });
        assert!(preview.start(path, "job_preview_fixture", 70).is_err());
        assert_eq!(preview.state(), DraftPreviewState::Stopped);
    }

    #[test]
    fn focus_start_stops_an_active_preview() {
        let (_temp, path) = valid_draft();
        let mut preview = PreviewAudioCoordinator::new(MockAudio::default());
        preview.start(path, "job_preview_fixture", 70).unwrap();
        stop_for_focus_start(&mut preview).unwrap();
        assert_eq!(preview.state(), DraftPreviewState::Stopped);
    }

    #[test]
    fn preview_writes_no_session_history() {
        use persistence::{PreferencesRepository, SessionHistoryStore};

        let storage = tempfile::tempdir().unwrap();
        let mut preferences =
            PreferencesRepository::open(storage.path().join("preferences.sqlite3")).unwrap();
        let before = preferences.recent_session_history(10).unwrap();
        let (_draft_dir, path) = valid_draft();
        let mut preview = PreviewAudioCoordinator::new(MockAudio::default());
        preview.start(path, "job_preview_fixture", 70).unwrap();
        assert_eq!(preferences.recent_session_history(10).unwrap(), before);
    }
}
