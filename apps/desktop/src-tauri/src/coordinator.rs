use audio_engine::{
    AudioError, AudioFacade, AudioIntensity, PlaybackSource, PlaybackSourceKind, PlaybackState,
    SourceLabel,
};
use domain::{
    Activity, Intensity, MasterVolume, Session, SessionError, SessionSnapshot, SessionStatus,
    SessionType,
};
use persistence::{
    OnboardingPreferences, OnboardingStore, PersistenceError, PreferenceStore, SessionEndReason,
    SessionFocusOutcome, SessionHistoryRecord, SessionHistoryStore, SessionSoundEnjoyment,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum CoordinatorError {
    #[error(transparent)]
    Domain(#[from] SessionError),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(
        "preference write failed ({persistence}); restoring the previous audio setting also failed ({audio_rollback})"
    )]
    PersistenceAndAudioRollback {
        persistence: PersistenceError,
        audio_rollback: AudioError,
    },
}

/// Coordinates domain, native audio, and durable preferences as one command
/// boundary. Candidate domain state is committed only after audio and SQLite
/// accept the operation.
pub(crate) struct SessionAudioCoordinator<
    A: AudioFacade,
    P: PreferenceStore + OnboardingStore + SessionHistoryStore,
> {
    session: Session,
    audio: A,
    preferences: P,
    active_history_id: Option<String>,
    active_source: Option<PlaybackSource>,
}

#[allow(dead_code)]
impl<A: AudioFacade, P: PreferenceStore + OnboardingStore + SessionHistoryStore>
    SessionAudioCoordinator<A, P>
{
    pub(crate) fn new(session: Session, audio: A, preferences: P) -> Self {
        Self {
            session,
            audio,
            preferences,
            active_history_id: None,
            active_source: None,
        }
    }

    pub(crate) fn restore(mut audio: A, mut preferences: P) -> Result<Self, CoordinatorError> {
        preferences.reconcile_interrupted_session_history(0)?;
        let activity = preferences.load_last_activity()?.unwrap_or_default();
        let intensity = preferences.load_intensity(activity)?.unwrap_or_default();
        let session_type = preferences.load_session_type(activity)?.unwrap_or_default();
        audio.set_intensity(to_audio(intensity))?;
        let volume = preferences.load_master_volume()?.unwrap_or_default();
        audio.set_master_volume(volume.percent())?;
        Ok(Self::new(
            Session::new(activity, intensity, session_type)?,
            audio,
            preferences,
        ))
    }

    pub(crate) fn restore_at(
        mut audio: A,
        mut preferences: P,
        reconciled_at: u64,
    ) -> Result<Self, CoordinatorError> {
        preferences.reconcile_interrupted_session_history(reconciled_at)?;
        let activity = preferences.load_last_activity()?.unwrap_or_default();
        let intensity = preferences.load_intensity(activity)?.unwrap_or_default();
        let session_type = preferences.load_session_type(activity)?.unwrap_or_default();
        audio.set_intensity(to_audio(intensity))?;
        let volume = preferences.load_master_volume()?.unwrap_or_default();
        audio.set_master_volume(volume.percent())?;
        Ok(Self::new(
            Session::new(activity, intensity, session_type)?,
            audio,
            preferences,
        ))
    }

    #[cfg(test)]
    pub(crate) fn start(&mut self, now: u64) -> Result<(), CoordinatorError> {
        self.start_with_source(now, PlaybackSource::TestTone)
    }

    pub(crate) fn start_with_source(
        &mut self,
        now: u64,
        source: PlaybackSource,
    ) -> Result<(), CoordinatorError> {
        self.start_recorded(now, now, source)
    }

    pub(crate) fn start_recorded(
        &mut self,
        now: u64,
        started_at: u64,
        source: PlaybackSource,
    ) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        candidate.start(now)?;
        let snapshot = candidate.snapshot(now);
        self.transition_audio(&snapshot, Some(source.clone()))?;
        match self.preferences.begin_session_history(
            snapshot.activity,
            snapshot.intensity,
            snapshot.kind,
            started_at,
        ) {
            Ok(record) => {
                self.session = candidate;
                self.active_history_id = Some(record.id);
                self.active_source = Some(source);
                Ok(())
            }
            Err(error) => match self.audio.stop() {
                Ok(()) => Err(error.into()),
                Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                    persistence: error,
                    audio_rollback,
                }),
            },
        }
    }

    /// Starts an inactive session for an explicitly chosen activity without
    /// committing the activity or its recent-playback side effects until audio
    /// accepts the prepared source.
    pub(crate) fn start_favorite_with_source(
        &mut self,
        now: u64,
        started_at: u64,
        activity: Activity,
        source: PlaybackSource,
    ) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        candidate.select_activity(activity)?;
        let intensity = self
            .preferences
            .load_intensity(activity)?
            .unwrap_or_default();
        let session_type = self
            .preferences
            .load_session_type(activity)?
            .unwrap_or_default();
        candidate.set_intensity(intensity);
        candidate.set_session_type(session_type)?;
        candidate.start(now)?;

        let previous_intensity = self.session.intensity();
        self.audio.set_intensity(to_audio(intensity))?;
        let snapshot = candidate.snapshot(now);
        if let Err(error) = self.transition_audio(&snapshot, Some(source.clone())) {
            let _ = self.audio.set_intensity(to_audio(previous_intensity));
            return Err(error.into());
        }
        match self.preferences.save_last_activity_with_history(
            activity,
            intensity,
            session_type,
            started_at,
        ) {
            Ok(record) => self.active_history_id = Some(record.id),
            Err(error) => {
                let stop_result = self.audio.stop();
                let _ = self.audio.set_intensity(to_audio(previous_intensity));
                return match stop_result {
                    Ok(()) => Err(error.into()),
                    Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                        persistence: error,
                        audio_rollback,
                    }),
                };
            }
        }
        self.session = candidate;
        self.active_source = Some(source);
        Ok(())
    }

    pub(crate) fn onboarding_preferences(
        &mut self,
    ) -> Result<OnboardingPreferences, CoordinatorError> {
        Ok(self.preferences.onboarding_preferences()?)
    }

    /// Start audio first, then atomically persist all onboarding preferences. On
    /// persistence failure, stop audio and leave the domain session untouched so
    /// the caller can retry without a falsely completed first run.
    pub(crate) fn complete_onboarding(
        &mut self,
        now: u64,
        started_at: u64,
        intensity: Intensity,
        genres: &[String],
        source: PlaybackSource,
    ) -> Result<(), CoordinatorError> {
        let mut candidate = Session::new(
            Activity::DeepWork,
            intensity,
            SessionType::Countdown { seconds: 1_800 },
        )?;
        candidate.start(now)?;
        let snapshot = candidate.snapshot(now);
        self.transition_audio(&snapshot, Some(source.clone()))?;
        match self
            .preferences
            .complete_onboarding_with_history(intensity, genres, started_at)
        {
            Ok(record) => self.active_history_id = Some(record.id),
            Err(error) => {
                return match self.audio.stop() {
                    Ok(()) => Err(error.into()),
                    Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                        persistence: error,
                        audio_rollback,
                    }),
                };
            }
        }
        self.session = candidate;
        self.active_source = Some(source);
        Ok(())
    }

    pub(crate) fn pause(&mut self, now: u64) -> Result<(), CoordinatorError> {
        self.apply_transport(|candidate| candidate.pause(now))
    }

    pub(crate) fn resume(&mut self, now: u64) -> Result<(), CoordinatorError> {
        self.apply_transport(|candidate| candidate.resume(now))
    }

    pub(crate) fn stop(&mut self, now: u64) -> Result<(), CoordinatorError> {
        self.stop_at(now, now)
    }
    pub(crate) fn stop_at(&mut self, now: u64, ended_at: u64) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        candidate.stop(now)?;
        let snapshot = candidate.snapshot(now);
        self.transition_audio(&snapshot, None)?;
        if let Some(id) = self.active_history_id.as_deref() {
            if let Err(persistence) = self.preferences.finalize_session_history(
                id,
                SessionEndReason::Stopped,
                ended_at,
                snapshot.focus_elapsed_seconds,
            ) {
                return match self.restore_audio(&self.session.snapshot(now)) {
                    Ok(()) => Err(persistence.into()),
                    Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                        persistence,
                        audio_rollback,
                    }),
                };
            }
            self.active_history_id = None;
            self.active_source = None;
        }
        self.session = candidate;
        Ok(())
    }

    pub(crate) fn select_activity(&mut self, activity: Activity) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        candidate.select_activity(activity)?;
        let target_intensity = self
            .preferences
            .load_intensity(activity)?
            .unwrap_or_default();
        let target_session_type = self
            .preferences
            .load_session_type(activity)?
            .unwrap_or_default();
        candidate.set_intensity(target_intensity);
        candidate.set_session_type(target_session_type)?;

        let previous_intensity = self.session.intensity();
        self.audio.set_intensity(to_audio(target_intensity))?;
        if let Err(persistence) = self.preferences.save_last_activity(activity) {
            return match self.audio.set_intensity(to_audio(previous_intensity)) {
                Ok(()) => Err(persistence.into()),
                Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                    persistence,
                    audio_rollback,
                }),
            };
        }

        self.session = candidate;
        Ok(())
    }

    pub(crate) fn set_intensity(&mut self, intensity: Intensity) -> Result<(), CoordinatorError> {
        let previous = self.session.intensity();
        let activity = self.session.activity();
        self.audio.set_intensity(to_audio(intensity))?;
        if let Err(persistence) = self.preferences.save_intensity(activity, intensity) {
            return match self.audio.set_intensity(to_audio(previous)) {
                Ok(()) => Err(persistence.into()),
                Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                    persistence,
                    audio_rollback,
                }),
            };
        }

        self.session.set_intensity(intensity);
        Ok(())
    }

    /// Global post-DSP master gain. It deliberately never mutates session/timer/intensity state.
    pub(crate) fn set_master_volume(
        &mut self,
        volume: MasterVolume,
    ) -> Result<(), CoordinatorError> {
        let previous = self.audio.master_volume();
        self.audio.set_master_volume(volume.percent())?;
        if let Err(persistence) = self.preferences.save_master_volume(volume) {
            return match self.audio.set_master_volume(previous) {
                Ok(()) => Err(persistence.into()),
                Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                    persistence,
                    audio_rollback,
                }),
            };
        }
        Ok(())
    }

    pub(crate) fn master_volume(&self) -> MasterVolume {
        MasterVolume::new(self.audio.master_volume())
            .expect("audio controller keeps volume bounded")
    }

    pub(crate) fn set_session_type(
        &mut self,
        session_type: SessionType,
    ) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        candidate.set_session_type(session_type)?;
        self.preferences
            .save_session_type(candidate.activity(), session_type)?;
        self.session = candidate;
        Ok(())
    }

    pub(crate) fn tick(&mut self, now: u64) -> Result<SessionSnapshot, CoordinatorError> {
        self.tick_at(now, now)
    }
    pub(crate) fn tick_at(
        &mut self,
        now: u64,
        ended_at: u64,
    ) -> Result<SessionSnapshot, CoordinatorError> {
        let mut candidate = self.session.clone();
        let snapshot = candidate.tick(now);
        self.transition_audio(&snapshot, None)?;
        if snapshot.status == SessionStatus::Expired {
            if let Some(id) = self.active_history_id.as_deref() {
                if let Err(persistence) = self.preferences.finalize_session_history(
                    id,
                    SessionEndReason::Expired,
                    ended_at,
                    snapshot.focus_elapsed_seconds,
                ) {
                    return match self.restore_audio(&self.session.snapshot(now)) {
                        Ok(()) => Err(persistence.into()),
                        Err(audio_rollback) => Err(CoordinatorError::PersistenceAndAudioRollback {
                            persistence,
                            audio_rollback,
                        }),
                    };
                }
                self.active_history_id = None;
                self.active_source = None;
            }
        }
        self.session = candidate;
        Ok(snapshot)
    }

    fn apply_transport(
        &mut self,
        operation: impl FnOnce(&mut Session) -> Result<(), SessionError>,
    ) -> Result<(), CoordinatorError> {
        self.apply_transport_with_source(operation, None)
    }

    fn apply_transport_with_source(
        &mut self,
        operation: impl FnOnce(&mut Session) -> Result<(), SessionError>,
        source: Option<PlaybackSource>,
    ) -> Result<(), CoordinatorError> {
        let mut candidate = self.session.clone();
        operation(&mut candidate)?;
        let snapshot = candidate.snapshot(0);
        self.transition_audio(&snapshot, source)?;
        self.session = candidate;
        Ok(())
    }

    fn transition_audio(
        &mut self,
        snapshot: &SessionSnapshot,
        source: Option<PlaybackSource>,
    ) -> Result<(), AudioError> {
        let target = match (snapshot.status, snapshot.phase) {
            (SessionStatus::Playing, Some(domain::SessionPhase::Work)) => PlaybackState::Playing,
            (SessionStatus::Playing, Some(domain::SessionPhase::Break))
            | (SessionStatus::Paused, _) => PlaybackState::Paused,
            _ => PlaybackState::Stopped,
        };
        match (self.audio.state(), target) {
            (PlaybackState::Stopped, PlaybackState::Stopped)
            | (PlaybackState::Playing, PlaybackState::Playing)
            | (PlaybackState::Paused, PlaybackState::Paused) => Ok(()),
            (PlaybackState::Stopped, PlaybackState::Playing) => self.audio.start_with_source(
                source.unwrap_or(PlaybackSource::TestTone),
                to_audio(snapshot.intensity),
            ),
            (PlaybackState::Paused, PlaybackState::Playing) => self.audio.resume(),
            (PlaybackState::Playing, PlaybackState::Paused) => self.audio.pause(),
            (_, PlaybackState::Stopped) => self.audio.stop(),
            (PlaybackState::Stopped, PlaybackState::Paused) => Err(AudioError::InvalidTransition {
                state: PlaybackState::Stopped,
                operation: "pause",
            }),
        }
    }

    /// Restores the exact prior transport state after a durable finalization
    /// failure. A paused source needs a start followed by pause because the
    /// normal transition table intentionally rejects stopped -> paused.
    fn restore_audio(&mut self, snapshot: &SessionSnapshot) -> Result<(), AudioError> {
        if snapshot.status == SessionStatus::Paused && self.audio.state() == PlaybackState::Stopped
        {
            self.audio.start_with_source(
                self.active_source
                    .clone()
                    .unwrap_or(PlaybackSource::TestTone),
                to_audio(snapshot.intensity),
            )?;
            return self.audio.pause();
        }
        self.transition_audio(snapshot, self.active_source.clone())
    }

    pub(crate) fn snapshot(&self, now: u64) -> SessionSnapshot {
        self.session.snapshot(now)
    }

    pub(crate) fn source_label(&self) -> SourceLabel {
        self.audio.source_label()
    }

    pub(crate) fn source_kind(&self) -> PlaybackSourceKind {
        self.audio.source_kind()
    }

    pub(crate) fn recent_history(
        &mut self,
        limit: usize,
    ) -> Result<Vec<SessionHistoryRecord>, CoordinatorError> {
        Ok(self.preferences.recent_session_history(limit)?)
    }

    pub(crate) fn save_session_ratings(
        &mut self,
        id: &str,
        focus: Option<SessionFocusOutcome>,
        enjoyment: Option<SessionSoundEnjoyment>,
    ) -> Result<(), CoordinatorError> {
        Ok(self
            .preferences
            .save_session_ratings(id, focus, enjoyment)?)
    }

    pub(crate) fn navigation_available(&self) -> bool {
        self.audio.navigation_available()
    }
    pub(crate) fn next_track(&mut self) -> Result<(), CoordinatorError> {
        Ok(self.audio.navigate_next()?)
    }
    pub(crate) fn previous_track(&mut self) -> Result<(), CoordinatorError> {
        Ok(self.audio.navigate_previous()?)
    }
}

fn to_audio(intensity: Intensity) -> AudioIntensity {
    match intensity {
        Intensity::Off => AudioIntensity::Off,
        Intensity::Low => AudioIntensity::Low,
        Intensity::Medium => AudioIntensity::Medium,
        Intensity::High => AudioIntensity::High,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use audio_engine::PlaybackState;
    use domain::SessionPhase;
    use persistence::{SessionFocusOutcome, SessionHistoryRecord, SessionSoundEnjoyment};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Operation {
        Start,
        Pause,
        Resume,
        Stop,
        Intensity,
        Volume,
    }

    struct MockAudio {
        state: PlaybackState,
        intensity: AudioIntensity,
        volume: u8,
        source_label: SourceLabel,
        fail_next: Option<Operation>,
        operations: Vec<Operation>,
    }

    impl Default for MockAudio {
        fn default() -> Self {
            Self {
                state: PlaybackState::Stopped,
                intensity: AudioIntensity::Medium,
                volume: 70,
                source_label: SourceLabel::test_fallback(),
                fail_next: None,
                operations: Vec::new(),
            }
        }
    }

    impl MockAudio {
        fn fail(&mut self, operation: Operation) -> Result<(), AudioError> {
            self.operations.push(operation);
            if self.fail_next == Some(operation) {
                self.fail_next = None;
                Err(AudioError::InjectedFailure)
            } else {
                Ok(())
            }
        }
    }

    impl AudioFacade for MockAudio {
        fn start(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
            self.start_with_source(PlaybackSource::TestTone, intensity)
        }

        fn start_with_source(
            &mut self,
            source: PlaybackSource,
            intensity: AudioIntensity,
        ) -> Result<(), AudioError> {
            self.fail(Operation::Start)?;
            self.state = PlaybackState::Playing;
            self.intensity = intensity;
            self.source_label = source.label();
            Ok(())
        }

        fn pause(&mut self) -> Result<(), AudioError> {
            self.fail(Operation::Pause)?;
            self.state = PlaybackState::Paused;
            Ok(())
        }

        fn resume(&mut self) -> Result<(), AudioError> {
            self.fail(Operation::Resume)?;
            self.state = PlaybackState::Playing;
            Ok(())
        }

        fn stop(&mut self) -> Result<(), AudioError> {
            self.fail(Operation::Stop)?;
            self.state = PlaybackState::Stopped;
            Ok(())
        }

        fn set_intensity(&mut self, intensity: AudioIntensity) -> Result<(), AudioError> {
            self.fail(Operation::Intensity)?;
            self.intensity = intensity;
            Ok(())
        }
        fn set_master_volume(&mut self, percent: u8) -> Result<(), AudioError> {
            self.fail(Operation::Volume)?;
            self.volume = percent;
            Ok(())
        }
        fn master_volume(&self) -> u8 {
            self.volume
        }

        fn state(&self) -> PlaybackState {
            self.state
        }

        fn intensity(&self) -> AudioIntensity {
            self.intensity
        }

        fn source_label(&self) -> SourceLabel {
            self.source_label.clone()
        }
    }

    #[derive(Default)]
    struct MockPreferences {
        last_activity: Option<Activity>,
        intensities: HashMap<Activity, Intensity>,
        session_types: HashMap<Activity, SessionType>,
        master_volume: Option<MasterVolume>,
        fail_next_save: bool,
        saves: usize,
        history: Vec<SessionHistoryRecord>,
    }

    impl PreferenceStore for MockPreferences {
        fn load_last_activity(&mut self) -> Result<Option<Activity>, PersistenceError> {
            Ok(self.last_activity)
        }

        fn save_last_activity(&mut self, activity: Activity) -> Result<(), PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".to_owned()));
            }
            self.last_activity = Some(activity);
            self.saves += 1;
            Ok(())
        }

        fn load_intensity(
            &mut self,
            activity: Activity,
        ) -> Result<Option<Intensity>, PersistenceError> {
            Ok(self.intensities.get(&activity).copied())
        }

        fn save_intensity(
            &mut self,
            activity: Activity,
            intensity: Intensity,
        ) -> Result<(), PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".to_owned()));
            }
            self.intensities.insert(activity, intensity);
            self.saves += 1;
            Ok(())
        }
        fn load_master_volume(&mut self) -> Result<Option<MasterVolume>, PersistenceError> {
            Ok(self.master_volume)
        }
        fn save_master_volume(&mut self, volume: MasterVolume) -> Result<(), PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".to_owned()));
            }
            self.master_volume = Some(volume);
            self.saves += 1;
            Ok(())
        }

        fn load_session_type(
            &mut self,
            activity: Activity,
        ) -> Result<Option<SessionType>, PersistenceError> {
            Ok(self.session_types.get(&activity).copied())
        }

        fn save_session_type(
            &mut self,
            activity: Activity,
            session_type: SessionType,
        ) -> Result<(), PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".to_owned()));
            }
            self.session_types.insert(activity, session_type);
            self.saves += 1;
            Ok(())
        }
    }

    impl OnboardingStore for MockPreferences {
        fn onboarding_preferences(&mut self) -> Result<OnboardingPreferences, PersistenceError> {
            Ok(OnboardingPreferences {
                completed: false,
                intensity: Intensity::Medium,
                genres: vec![],
            })
        }
        fn complete_onboarding(
            &mut self,
            _: Intensity,
            _: &[String],
        ) -> Result<(), PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".into()));
            }
            Ok(())
        }
    }

    impl SessionHistoryStore for MockPreferences {
        fn complete_onboarding_with_history(
            &mut self,
            intensity: Intensity,
            _: &[String],
            started_at: u64,
        ) -> Result<SessionHistoryRecord, PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".into()));
            }
            self.begin_session_history(
                Activity::DeepWork,
                intensity,
                SessionType::Countdown { seconds: 1_800 },
                started_at,
            )
        }
        fn save_last_activity_with_history(
            &mut self,
            activity: Activity,
            intensity: Intensity,
            session_type: SessionType,
            started_at: u64,
        ) -> Result<SessionHistoryRecord, PersistenceError> {
            if self.fail_next_save {
                self.fail_next_save = false;
                return Err(PersistenceError::Storage("injected".into()));
            }
            self.last_activity = Some(activity);
            self.begin_session_history(activity, intensity, session_type, started_at)
        }
        fn begin_session_history(
            &mut self,
            activity: Activity,
            intensity: Intensity,
            session_type: SessionType,
            started_at: u64,
        ) -> Result<SessionHistoryRecord, PersistenceError> {
            if self.history.iter().any(|row| row.ended_at.is_none()) {
                return Err(PersistenceError::Storage("active history".into()));
            }
            let record = SessionHistoryRecord {
                id: format!("{:032x}", self.history.len() + 1),
                activity,
                intensity,
                session_type,
                started_at,
                ended_at: None,
                end_reason: None,
                focus_seconds: None,
                focus_outcome: None,
                sound_enjoyment: None,
            };
            self.history.push(record.clone());
            Ok(record)
        }
        fn finalize_session_history(
            &mut self,
            id: &str,
            reason: SessionEndReason,
            ended_at: u64,
            focus_seconds: u64,
        ) -> Result<(), PersistenceError> {
            let row = self
                .history
                .iter_mut()
                .find(|row| row.id == id && row.ended_at.is_none())
                .ok_or_else(|| PersistenceError::UnknownSessionHistory(id.into()))?;
            row.ended_at = Some(ended_at);
            row.end_reason = Some(reason);
            row.focus_seconds = Some(focus_seconds);
            Ok(())
        }
        fn reconcile_interrupted_session_history(
            &mut self,
            ended_at: u64,
        ) -> Result<(), PersistenceError> {
            for row in &mut self.history {
                if row.ended_at.is_none() {
                    row.ended_at = Some(ended_at);
                    row.end_reason = Some(SessionEndReason::Interrupted);
                }
            }
            Ok(())
        }
        fn recent_session_history(
            &mut self,
            _: usize,
        ) -> Result<Vec<SessionHistoryRecord>, PersistenceError> {
            Ok(self.history.clone())
        }
        fn save_session_ratings(
            &mut self,
            _: &str,
            _: Option<SessionFocusOutcome>,
            _: Option<SessionSoundEnjoyment>,
        ) -> Result<(), PersistenceError> {
            Ok(())
        }
    }

    fn coordinator() -> SessionAudioCoordinator<MockAudio, MockPreferences> {
        SessionAudioCoordinator::new(
            Session::default(),
            MockAudio::default(),
            MockPreferences::default(),
        )
    }

    fn interval_session() -> Session {
        Session::new(
            Activity::DeepWork,
            Intensity::Medium,
            SessionType::Interval {
                work_seconds: 60,
                break_seconds: 60,
                repeats: 3,
            },
        )
        .unwrap()
    }

    fn interval_coordinator() -> SessionAudioCoordinator<MockAudio, MockPreferences> {
        SessionAudioCoordinator::new(
            interval_session(),
            MockAudio::default(),
            MockPreferences::default(),
        )
    }

    fn installed_source() -> PlaybackSource {
        let label = SourceLabel {
            pack_id: "test.pack".to_owned(),
            pack_title: "Test Pack".to_owned(),
            item_id: "focus-item".to_owned(),
            item_title: "Focus Item".to_owned(),
            variant_id: "source".to_owned(),
        };
        PlaybackSource::Installed(
            audio_engine::DecodedProgram::new(vec![audio_engine::DecodedTrack {
                sample_rate_hz: 8_000,
                channels: 1,
                samples: vec![0.0; 8_000].into(),
                regions: vec![audio_engine::AuthoredRegion {
                    kind: audio_engine::AuthoredRegionKind::Loop,
                    start_seconds: 0.1,
                    end_seconds: 0.9,
                }],
                label,
            }])
            .unwrap(),
        )
    }

    #[test]
    fn restore_uses_saved_activity_and_its_intensity() {
        let mut preferences = MockPreferences {
            last_activity: Some(Activity::Learning),
            ..MockPreferences::default()
        };
        preferences
            .intensities
            .insert(Activity::Learning, Intensity::Low);
        preferences.session_types.insert(
            Activity::Learning,
            SessionType::Countdown { seconds: 1_500 },
        );

        let coordinator =
            SessionAudioCoordinator::restore(MockAudio::default(), preferences).unwrap();
        assert_eq!(coordinator.snapshot(0).activity, Activity::Learning);
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::Low);
        assert_eq!(coordinator.audio.intensity(), AudioIntensity::Low);
        assert_eq!(
            coordinator.snapshot(0).kind,
            SessionType::Countdown { seconds: 1_500 }
        );
    }

    #[test]
    fn transport_commands_keep_domain_and_audio_aligned() {
        let mut coordinator = coordinator();
        coordinator.start(0).unwrap();
        assert_eq!(coordinator.snapshot(5).status, SessionStatus::Playing);
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);
        coordinator.pause(5).unwrap();
        assert_eq!(coordinator.snapshot(100).status, SessionStatus::Paused);
        assert_eq!(coordinator.audio.state(), PlaybackState::Paused);
        coordinator.resume(10).unwrap();
        coordinator.stop(15).unwrap();
        assert_eq!(coordinator.snapshot(20).status, SessionStatus::Stopped);
        assert_eq!(coordinator.audio.state(), PlaybackState::Stopped);
    }

    #[test]
    fn start_failure_rolls_domain_back_to_idle() {
        let audio = MockAudio {
            fail_next: Some(Operation::Start),
            ..MockAudio::default()
        };
        let mut coordinator =
            SessionAudioCoordinator::new(Session::default(), audio, MockPreferences::default());

        assert!(coordinator.start(0).is_err());
        assert_eq!(coordinator.snapshot(10).status, SessionStatus::Idle);
        assert_eq!(coordinator.audio.state(), PlaybackState::Stopped);
    }

    #[test]
    fn onboarding_commits_the_requested_countdown_only_after_audio_starts() {
        let mut coordinator = coordinator();

        coordinator
            .complete_onboarding(
                100,
                1_000,
                Intensity::High,
                &["drone".to_owned(), "nature".to_owned()],
                installed_source(),
            )
            .unwrap();

        let snapshot = coordinator.snapshot(100);
        assert_eq!(snapshot.status, SessionStatus::Playing);
        assert_eq!(snapshot.activity, Activity::DeepWork);
        assert_eq!(snapshot.intensity, Intensity::High);
        assert_eq!(snapshot.kind, SessionType::Countdown { seconds: 1_800 });
        assert_eq!(coordinator.audio.operations, vec![Operation::Start]);
    }

    #[test]
    fn onboarding_persistence_failure_stops_audio_and_keeps_the_old_session() {
        let preferences = MockPreferences {
            fail_next_save: true,
            ..MockPreferences::default()
        };
        let mut coordinator =
            SessionAudioCoordinator::new(Session::default(), MockAudio::default(), preferences);

        assert!(coordinator
            .complete_onboarding(
                100,
                1_000,
                Intensity::Low,
                &["piano".to_owned()],
                installed_source(),
            )
            .is_err());

        assert_eq!(coordinator.snapshot(100).status, SessionStatus::Idle);
        assert_eq!(coordinator.audio.state(), PlaybackState::Stopped);
        assert_eq!(
            coordinator.audio.operations,
            vec![Operation::Start, Operation::Stop]
        );
    }

    #[test]
    fn installed_source_label_commits_only_after_successful_start() {
        let mut coordinator = coordinator();
        coordinator
            .start_with_source(0, installed_source())
            .unwrap();
        assert_eq!(coordinator.source_label().item_id, "focus-item");

        let audio = MockAudio {
            fail_next: Some(Operation::Start),
            ..MockAudio::default()
        };
        let mut failed =
            SessionAudioCoordinator::new(Session::default(), audio, MockPreferences::default());
        assert!(failed.start_with_source(0, installed_source()).is_err());
        assert_eq!(failed.snapshot(1).status, SessionStatus::Idle);
        assert_eq!(failed.source_label(), SourceLabel::test_fallback());
    }

    #[test]
    fn pause_failure_restores_playing_clock_state() {
        let mut coordinator = coordinator();
        coordinator.start(0).unwrap();
        coordinator.audio.fail_next = Some(Operation::Pause);

        assert!(coordinator.pause(5).is_err());
        assert_eq!(coordinator.snapshot(10).status, SessionStatus::Playing);
        assert_eq!(coordinator.snapshot(10).focus_elapsed_seconds, 10);
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);
    }

    #[test]
    fn intensity_audio_failure_is_not_persisted() {
        let mut coordinator = coordinator();
        coordinator.audio.fail_next = Some(Operation::Intensity);

        assert!(coordinator.set_intensity(Intensity::High).is_err());
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::Medium);
        assert_eq!(coordinator.audio.intensity(), AudioIntensity::Medium);
        assert_eq!(coordinator.preferences.saves, 0);
        assert!(!coordinator
            .preferences
            .intensities
            .contains_key(&Activity::DeepWork));
    }

    #[test]
    fn successful_intensity_change_is_persisted_for_current_activity() {
        let mut coordinator = coordinator();
        coordinator.set_intensity(Intensity::High).unwrap();
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::High);
        assert_eq!(
            coordinator.preferences.intensities.get(&Activity::DeepWork),
            Some(&Intensity::High)
        );
    }

    #[test]
    fn master_volume_is_global_and_does_not_change_intensity_timer_or_transport() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        coordinator.set_intensity(Intensity::High).unwrap();
        let before = coordinator.snapshot(30);
        coordinator
            .set_master_volume(MasterVolume::new(35).unwrap())
            .unwrap();
        let after = coordinator.snapshot(30);
        assert_eq!(coordinator.master_volume().percent(), 35);
        assert_eq!(after.intensity, before.intensity);
        assert_eq!(after.kind, before.kind);
        assert_eq!(after.status, before.status);
        assert_eq!(after.focus_elapsed_seconds, before.focus_elapsed_seconds);
        coordinator.set_intensity(Intensity::Low).unwrap();
        assert_eq!(coordinator.master_volume().percent(), 35);
    }

    #[test]
    fn failed_intensity_persistence_rolls_audio_and_domain_back() {
        let mut coordinator = coordinator();
        coordinator.preferences.fail_next_save = true;

        assert!(coordinator.set_intensity(Intensity::High).is_err());
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::Medium);
        assert_eq!(coordinator.audio.intensity(), AudioIntensity::Medium);
        assert_eq!(coordinator.preferences.saves, 0);
    }

    #[test]
    fn selecting_activity_restores_saved_intensity_and_persists_selection() {
        let mut coordinator = coordinator();
        coordinator
            .preferences
            .intensities
            .insert(Activity::Creativity, Intensity::Low);
        coordinator.preferences.session_types.insert(
            Activity::Creativity,
            SessionType::Interval {
                work_seconds: 1_500,
                break_seconds: 300,
                repeats: 4,
            },
        );

        coordinator.select_activity(Activity::Creativity).unwrap();
        assert_eq!(coordinator.snapshot(0).activity, Activity::Creativity);
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::Low);
        assert_eq!(coordinator.audio.intensity(), AudioIntensity::Low);
        assert_eq!(
            coordinator.preferences.last_activity,
            Some(Activity::Creativity)
        );
        assert!(matches!(
            coordinator.snapshot(0).kind,
            SessionType::Interval { repeats: 4, .. }
        ));
    }

    #[test]
    fn active_activity_change_is_rejected_without_audio_or_persistence_change() {
        let mut coordinator = coordinator();
        coordinator.start(0).unwrap();
        let previous_audio = coordinator.audio.intensity();

        assert!(coordinator.select_activity(Activity::Motivation).is_err());
        assert_eq!(coordinator.snapshot(1).activity, Activity::DeepWork);
        assert_eq!(coordinator.audio.intensity(), previous_audio);
        assert_eq!(coordinator.preferences.saves, 0);
    }

    #[test]
    fn failed_activity_persistence_restores_audio_and_domain() {
        let mut coordinator = coordinator();
        coordinator
            .preferences
            .intensities
            .insert(Activity::Learning, Intensity::High);
        coordinator.preferences.fail_next_save = true;

        assert!(coordinator.select_activity(Activity::Learning).is_err());
        assert_eq!(coordinator.snapshot(0).activity, Activity::DeepWork);
        assert_eq!(coordinator.snapshot(0).intensity, Intensity::Medium);
        assert_eq!(coordinator.audio.intensity(), AudioIntensity::Medium);
        assert_eq!(coordinator.preferences.last_activity, None);
    }

    #[test]
    fn expiry_stop_failure_restores_active_session() {
        let session = Session::new(
            Activity::DeepWork,
            Intensity::Medium,
            SessionType::Countdown { seconds: 60 },
        )
        .unwrap();
        let mut coordinator =
            SessionAudioCoordinator::new(session, MockAudio::default(), MockPreferences::default());
        coordinator.start(0).unwrap();
        coordinator.audio.fail_next = Some(Operation::Stop);

        assert!(coordinator.tick(60).is_err());
        assert_eq!(coordinator.snapshot(0).status, SessionStatus::Playing);
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);
    }

    #[test]
    fn timer_config_persists_before_domain_commit_and_is_inactive_only() {
        let mut coordinator = coordinator();
        let countdown = SessionType::Countdown { seconds: 1_500 };
        coordinator.set_session_type(countdown).unwrap();
        assert_eq!(coordinator.snapshot(0).kind, countdown);
        assert_eq!(
            coordinator
                .preferences
                .session_types
                .get(&Activity::DeepWork),
            Some(&countdown)
        );

        coordinator.preferences.fail_next_save = true;
        assert!(coordinator.set_session_type(SessionType::Infinite).is_err());
        assert_eq!(coordinator.snapshot(0).kind, countdown);
        coordinator.start(0).unwrap();
        assert!(coordinator.set_session_type(SessionType::Infinite).is_err());
        assert_eq!(coordinator.snapshot(1).kind, countdown);
    }

    #[test]
    fn interval_automatically_silences_break_resumes_work_and_stops_at_expiry() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);

        let rest = coordinator.tick(60).unwrap();
        assert_eq!(rest.phase, Some(SessionPhase::Break));
        assert_eq!(coordinator.audio.state(), PlaybackState::Paused);

        let work = coordinator.tick(120).unwrap();
        assert_eq!(work.phase, Some(SessionPhase::Work));
        assert_eq!(work.current_round, Some(2));
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);

        let expired = coordinator.tick(300).unwrap();
        assert_eq!(expired.status, SessionStatus::Expired);
        assert_eq!(coordinator.audio.state(), PlaybackState::Stopped);
    }

    #[test]
    fn user_pause_and_resume_during_break_do_not_duplicate_audio_commands() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        coordinator.tick(70).unwrap();
        let pause_calls = coordinator
            .audio
            .operations
            .iter()
            .filter(|operation| **operation == Operation::Pause)
            .count();
        coordinator.pause(80).unwrap();
        assert_eq!(coordinator.snapshot(1_000).phase, Some(SessionPhase::Break));
        assert_eq!(
            coordinator
                .audio
                .operations
                .iter()
                .filter(|operation| **operation == Operation::Pause)
                .count(),
            pause_calls
        );
        coordinator.resume(200).unwrap();
        assert_eq!(coordinator.audio.state(), PlaybackState::Paused);
        assert_eq!(
            coordinator
                .audio
                .operations
                .iter()
                .filter(|operation| **operation == Operation::Resume)
                .count(),
            0
        );
    }

    #[test]
    fn stop_from_interval_break_stops_silenced_audio() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        coordinator.tick(60).unwrap();
        coordinator.stop(70).unwrap();
        assert_eq!(coordinator.snapshot(1_000).status, SessionStatus::Stopped);
        assert_eq!(coordinator.audio.state(), PlaybackState::Stopped);
    }

    #[test]
    fn large_jump_uses_final_phase_without_replaying_intermediate_audio_actions() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        let operation_count = coordinator.audio.operations.len();
        let snapshot = coordinator.tick(130).unwrap();
        assert_eq!(snapshot.phase, Some(SessionPhase::Work));
        assert_eq!(snapshot.current_round, Some(2));
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);
        assert_eq!(coordinator.audio.operations.len(), operation_count);
    }

    #[test]
    fn automatic_break_transition_failure_restores_domain_and_audio() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        coordinator.audio.fail_next = Some(Operation::Pause);
        assert!(coordinator.tick(60).is_err());
        assert_eq!(coordinator.snapshot(0).phase, Some(SessionPhase::Work));
        assert_eq!(coordinator.snapshot(0).focus_elapsed_seconds, 0);
        assert_eq!(coordinator.audio.state(), PlaybackState::Playing);
    }

    #[test]
    fn automatic_work_resume_failure_restores_break_state() {
        let mut coordinator = interval_coordinator();
        coordinator.start(0).unwrap();
        coordinator.tick(60).unwrap();
        coordinator.audio.fail_next = Some(Operation::Resume);
        assert!(coordinator.tick(120).is_err());
        let restored = coordinator.snapshot(60);
        assert_eq!(restored.phase, Some(SessionPhase::Break));
        assert_eq!(restored.current_round, Some(1));
        assert_eq!(coordinator.audio.state(), PlaybackState::Paused);
    }
}
