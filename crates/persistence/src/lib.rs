//! Local, device-independent preference persistence.
//!
//! SQLite is compiled through rusqlite's bundled feature so the same repository
//! boundary works on supported desktop platforms without a system SQLite
//! dependency. This crate has no Tauri, audio-device, network, or UI knowledge.

use std::path::{Component, Path, PathBuf};

use domain::{Activity, Intensity, MasterVolume, SessionType, TrackEnjoyment, TrackFeedback};
use music_studio_domain::{
    StudioErrorCode, StudioFailureDetails, StudioJobId, StudioJobRecord, StudioJobState,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "activity_preferences",
        include_str!("../migrations/0001_activity_preferences.sql"),
    ),
    (
        2,
        "installed_catalogue",
        include_str!("../migrations/0002_installed_catalogue.sql"),
    ),
    (
        3,
        "activity_timer_preferences",
        include_str!("../migrations/0003_activity_timer_preferences.sql"),
    ),
    (
        4,
        "activity_genre_preferences",
        include_str!("../migrations/0004_activity_genre_preferences.sql"),
    ),
    (
        5,
        "item_activity_feedback",
        include_str!("../migrations/0005_item_activity_feedback.sql"),
    ),
    (
        6,
        "owner_waived_bundled_private_beta",
        include_str!("../migrations/0006_owner_waived_bundled_private_beta.sql"),
    ),
    (
        7,
        "activity_mood_preferences",
        include_str!("../migrations/0007_activity_mood_preferences.sql"),
    ),
    (
        8,
        "item_activity_enjoyment",
        include_str!("../migrations/0008_item_activity_enjoyment.sql"),
    ),
    (
        9,
        "master_volume",
        include_str!("../migrations/0009_master_volume.sql"),
    ),
    (
        10,
        "onboarding",
        include_str!("../migrations/0010_onboarding.sql"),
    ),
    (
        11,
        "session_history",
        include_str!("../migrations/0011_session_history.sql"),
    ),
    (
        12,
        "music_studio_jobs",
        include_str!("../migrations/0012_music_studio_jobs.sql"),
    ),
    (
        13,
        "generated_local_catalogue",
        include_str!("../migrations/0013_generated_local_catalogue.sql"),
    ),
    (
        14,
        "music_studio_job_artifacts",
        include_str!("../migrations/0014_music_studio_job_artifacts.sql"),
    ),
    (
        15,
        "generated_local_customer_metadata",
        include_str!("../migrations/0015_generated_local_customer_metadata.sql"),
    ),
    (
        16,
        "generated_local_customer_integrity",
        include_str!("../migrations/0016_generated_local_customer_integrity.sql"),
    ),
];
const ONBOARDING_GENRE_IDS: &[&str] = &[
    "atmospheric",
    "lo_fi",
    "electronic",
    "piano",
    "classical",
    "acoustic",
    "cinematic",
    "drone",
    "grooves",
    "post_rock",
    "nature",
];

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("SQLite preference operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("stored activity value is invalid: {0}")]
    InvalidActivity(String),
    #[error("stored intensity value is invalid: {0}")]
    InvalidIntensity(String),
    #[error("stored master volume is invalid: {0}")]
    InvalidMasterVolume(i64),
    #[error("stored timer configuration is invalid for {activity}: {reason}")]
    InvalidTimerConfig { activity: String, reason: String },
    #[error("stored genre identifier is invalid: {0}")]
    InvalidGenreId(String),
    #[error("stored mood identifier is invalid: {0}")]
    InvalidMoodId(String),
    #[error("stored item identifier is invalid: {0}")]
    InvalidItemId(String),
    #[error("stored track feedback value is invalid: {0}")]
    InvalidTrackFeedback(String),
    #[error("stored track enjoyment value is invalid: {0}")]
    InvalidTrackEnjoyment(String),
    #[error("onboarding must contain no more than three genres")]
    TooManyOnboardingGenres,
    #[error("installed item does not exist: {0}")]
    UnknownInstalledItem(String),
    #[error("preference storage failure: {0}")]
    Storage(String),
    #[error("stored session history row is corrupt: {0}")]
    InvalidSessionHistory(String),
    #[error("session history record does not exist or is not finalized: {0}")]
    UnknownSessionHistory(String),
    #[error("music studio job already exists: {0}")]
    DuplicateStudioJob(String),
    #[error("music studio attempt already exists: {0}")]
    DuplicateStudioAttempt(String),
    #[error("music studio job does not exist: {0}")]
    UnknownStudioJob(String),
    #[error("music studio job revision is stale: {0}")]
    StaleStudioJob(String),
    #[error("stored music studio job is corrupt")]
    InvalidStudioJob,
    #[error("music studio job update is invalid")]
    InvalidStudioJobUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHistoryRecord {
    pub id: String,
    pub activity: Activity,
    pub intensity: Intensity,
    pub session_type: SessionType,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub end_reason: Option<SessionEndReason>,
    pub focus_seconds: Option<u64>,
    pub focus_outcome: Option<SessionFocusOutcome>,
    pub sound_enjoyment: Option<SessionSoundEnjoyment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    Stopped,
    Expired,
    Interrupted,
}
impl SessionEndReason {
    fn key(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Expired => "expired",
            Self::Interrupted => "interrupted",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            "stopped" => Some(Self::Stopped),
            "expired" => Some(Self::Expired),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionFocusOutcome {
    HelpedFocus,
    Neutral,
    Distracting,
}
impl SessionFocusOutcome {
    fn key(self) -> &'static str {
        match self {
            Self::HelpedFocus => "helped_focus",
            Self::Neutral => "neutral",
            Self::Distracting => "distracting",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            "helped_focus" => Some(Self::HelpedFocus),
            "neutral" => Some(Self::Neutral),
            "distracting" => Some(Self::Distracting),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSoundEnjoyment {
    Liked,
    NotForMe,
}
impl SessionSoundEnjoyment {
    fn key(self) -> &'static str {
        match self {
            Self::Liked => "liked",
            Self::NotForMe => "not_for_me",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            "liked" => Some(Self::Liked),
            "not_for_me" => Some(Self::NotForMe),
            _ => None,
        }
    }
}

/// Session-level history is intentionally independent from item feedback and
/// cannot influence catalogue selection.
pub trait SessionHistoryStore: Send {
    /// Atomically records the first-run preferences and the active session row.
    fn complete_onboarding_with_history(
        &mut self,
        intensity: Intensity,
        genres: &[String],
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError>;
    /// Atomically changes the recent activity and creates the active session row.
    fn save_last_activity_with_history(
        &mut self,
        activity: Activity,
        intensity: Intensity,
        session_type: SessionType,
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError>;
    fn begin_session_history(
        &mut self,
        activity: Activity,
        intensity: Intensity,
        session_type: SessionType,
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError>;
    fn finalize_session_history(
        &mut self,
        id: &str,
        reason: SessionEndReason,
        ended_at: u64,
        focus_seconds: u64,
    ) -> Result<(), PersistenceError>;
    fn reconcile_interrupted_session_history(
        &mut self,
        ended_at: u64,
    ) -> Result<(), PersistenceError>;
    fn recent_session_history(
        &mut self,
        limit: usize,
    ) -> Result<Vec<SessionHistoryRecord>, PersistenceError>;
    fn save_session_ratings(
        &mut self,
        id: &str,
        focus_outcome: Option<SessionFocusOutcome>,
        sound_enjoyment: Option<SessionSoundEnjoyment>,
    ) -> Result<(), PersistenceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingPreferences {
    pub completed: bool,
    pub intensity: Intensity,
    pub genres: Vec<String>,
}

/// Global first-run choices are intentionally distinct from exact per-activity genre selection.
pub trait OnboardingStore: Send {
    fn onboarding_preferences(&mut self) -> Result<OnboardingPreferences, PersistenceError>;
    fn complete_onboarding(
        &mut self,
        intensity: Intensity,
        genres: &[String],
    ) -> Result<(), PersistenceError>;
}

/// Narrow durable boundary for user-owned per-activity genre choices. `None`
/// is the explicit Any-compatible-genre preference and removes a stored row.
pub trait GenrePreferenceStore: Send {
    fn load_genre_preference(
        &mut self,
        activity: Activity,
    ) -> Result<Option<String>, PersistenceError>;
    fn save_genre_preference(
        &mut self,
        activity: Activity,
        genre_id: &str,
    ) -> Result<(), PersistenceError>;
    fn clear_genre_preference(&mut self, activity: Activity) -> Result<(), PersistenceError>;
}

/// Narrow durable boundary for per-activity mood choices. `None` is Any compatible mood.
pub trait MoodPreferenceStore: Send {
    fn load_mood_preference(
        &mut self,
        activity: Activity,
    ) -> Result<Option<String>, PersistenceError>;
    fn save_mood_preference(
        &mut self,
        activity: Activity,
        mood_id: &str,
    ) -> Result<(), PersistenceError>;
    fn clear_mood_preference(&mut self, activity: Activity) -> Result<(), PersistenceError>;
}

/// Minimal durable boundary consumed by the application coordinator. Tests use
/// an in-memory implementation; production uses `PreferencesRepository`.
pub trait PreferenceStore: Send {
    fn load_last_activity(&mut self) -> Result<Option<Activity>, PersistenceError>;
    fn save_last_activity(&mut self, activity: Activity) -> Result<(), PersistenceError>;
    fn load_intensity(&mut self, activity: Activity)
        -> Result<Option<Intensity>, PersistenceError>;
    fn save_intensity(
        &mut self,
        activity: Activity,
        intensity: Intensity,
    ) -> Result<(), PersistenceError>;
    fn load_master_volume(&mut self) -> Result<Option<MasterVolume>, PersistenceError>;
    fn save_master_volume(&mut self, volume: MasterVolume) -> Result<(), PersistenceError>;
    fn load_session_type(
        &mut self,
        activity: Activity,
    ) -> Result<Option<SessionType>, PersistenceError>;
    fn save_session_type(
        &mut self,
        activity: Activity,
        session_type: SessionType,
    ) -> Result<(), PersistenceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledPackRecord {
    pub pack_id: String,
    pub title: String,
    pub version: String,
    pub manifest_sha256: String,
    pub archive_sha256: String,
    pub install_path: String,
    pub item_count: u32,
    pub status: String,
    pub canonical_manifest: String,
    pub created_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredItem {
    pub item_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredTaxonomyTerm {
    pub kind: String,
    pub term_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRegistration {
    pub pack: InstalledPackRecord,
    pub items: Vec<RegisteredItem>,
    pub taxonomy: Vec<RegisteredTaxonomyTerm>,
    pub generated_local_evidence: Option<GeneratedLocalEvidenceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedLocalEvidenceRecord {
    pub generation_job_id: String,
    pub evidence_json: String,
    pub created_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedLocalCustomerRecord {
    pub pack_id: String,
    pub item_id: String,
    pub title: String,
    pub activity: Activity,
    pub created_at_unix_seconds: i64,
}

pub trait CatalogueRegistry: Send {
    fn list_installed_packs(&mut self) -> Result<Vec<InstalledPackRecord>, PersistenceError>;
    fn find_installed_pack(
        &mut self,
        pack_id: &str,
    ) -> Result<Option<InstalledPackRecord>, PersistenceError>;
    fn find_existing_item_ids(
        &mut self,
        item_ids: &[String],
    ) -> Result<Vec<String>, PersistenceError>;
    fn register_pack(&mut self, registration: &PackRegistration) -> Result<(), PersistenceError>;
    fn replace_owner_waived_pack_preserving_feedback(
        &mut self,
        registration: &PackRegistration,
    ) -> Result<(), PersistenceError> {
        let _ = registration;
        Err(PersistenceError::Storage(
            "owner-waived pack upgrade is unavailable".into(),
        ))
    }
    fn register_generated_local_pack(
        &mut self,
        registration: &PackRegistration,
        customer: &GeneratedLocalCustomerRecord,
    ) -> Result<(), PersistenceError> {
        let _ = (registration, customer);
        Err(PersistenceError::Storage(
            "generated-local atomic registration is unavailable".into(),
        ))
    }
    fn find_generated_local_evidence(
        &mut self,
        pack_id: &str,
    ) -> Result<Option<GeneratedLocalEvidenceRecord>, PersistenceError>;
    fn list_generated_local_customers(
        &mut self,
    ) -> Result<Vec<GeneratedLocalCustomerRecord>, PersistenceError> {
        Err(PersistenceError::Storage(
            "generated-local customer metadata is unavailable".into(),
        ))
    }
    fn rename_generated_local_customer(
        &mut self,
        item_id: &str,
        title: &str,
    ) -> Result<(), PersistenceError> {
        let _ = (item_id, title);
        Err(PersistenceError::Storage(
            "generated-local customer metadata is unavailable".into(),
        ))
    }
    fn unregister_generated_local(
        &mut self,
        item_id: &str,
    ) -> Result<Option<InstalledPackRecord>, PersistenceError> {
        let _ = item_id;
        Err(PersistenceError::Storage(
            "generated-local customer metadata is unavailable".into(),
        ))
    }
}

/// Narrow per-item feedback boundary. It is implemented only by the
/// repository that owns the installed catalogue connection.
pub trait ItemFeedbackStore: Send {
    fn load_item_feedback(
        &mut self,
        activity: Activity,
        item_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, TrackFeedback>, PersistenceError>;
    fn save_item_feedback(
        &mut self,
        item_id: &str,
        activity: Activity,
        feedback: TrackFeedback,
    ) -> Result<(), PersistenceError>;
    fn clear_item_feedback(
        &mut self,
        item_id: &str,
        activity: Activity,
    ) -> Result<(), PersistenceError>;
    fn load_item_enjoyment(
        &mut self,
        activity: Activity,
        item_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, TrackEnjoyment>, PersistenceError>;
    fn save_item_enjoyment(
        &mut self,
        item_id: &str,
        activity: Activity,
        enjoyment: TrackEnjoyment,
    ) -> Result<(), PersistenceError>;
    fn clear_item_enjoyment(
        &mut self,
        item_id: &str,
        activity: Activity,
    ) -> Result<(), PersistenceError>;
}

/// Durable boundary for user-owned Music Studio jobs. Implementations must
/// validate reconstructed domain values before returning them to callers.
///
/// Recent-job queries are capped to keep startup and UI reads bounded.
pub const MAX_RECENT_STUDIO_JOBS: usize = 100;

/// Metadata is intentionally relative to the Music Studio data root.  Audio
/// bytes and process output never enter SQLite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioJobArtifact {
    pub job_id: StudioJobId,
    pub parent_job_id: Option<StudioJobId>,
    pub runtime_version: String,
    pub stage: String,
    pub output_relative_path: Option<String>,
    pub output_sha256: Option<String>,
    pub analysis_json: Option<String>,
    pub safe_error_code: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub trait StudioJobStore: Send {
    fn create_studio_job(&mut self, job: &StudioJobRecord) -> Result<(), PersistenceError>;
    fn load_studio_job(
        &mut self,
        job_id: &StudioJobId,
    ) -> Result<Option<StudioJobRecord>, PersistenceError>;
    fn recent_studio_jobs(
        &mut self,
        limit: usize,
    ) -> Result<Vec<StudioJobRecord>, PersistenceError>;
    fn transition_studio_job(
        &mut self,
        job_id: &StudioJobId,
        expected_revision: u64,
        next: StudioJobState,
        updated_at_ms: u64,
        failure: Option<StudioFailureDetails>,
    ) -> Result<StudioJobRecord, PersistenceError>;
    fn recover_studio_jobs(&mut self, recovered_at_ms: u64) -> Result<(), PersistenceError>;
    fn upsert_studio_job_artifact(
        &mut self,
        artifact: &StudioJobArtifact,
    ) -> Result<(), PersistenceError>;
    fn load_studio_job_artifact(
        &mut self,
        job_id: &StudioJobId,
    ) -> Result<Option<StudioJobArtifact>, PersistenceError>;
    fn remove_studio_job_artifact(&mut self, job_id: &StudioJobId) -> Result<(), PersistenceError>;
    fn remove_studio_job(&mut self, job_id: &StudioJobId) -> Result<(), PersistenceError>;
}

pub struct PreferencesRepository {
    connection: Connection,
}

impl PreferencesRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, PersistenceError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, PersistenceError> {
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        apply_migrations(&mut connection)?;
        Ok(Self { connection })
    }

    /// Rebase install paths after the application-data directory itself was
    /// atomically renamed during the product brand migration. Only the exact
    /// service-owned `content/packs/<pack-id>/<version-key>` shape is accepted;
    /// arbitrary absolute, relative, linked, or missing paths are rejected and
    /// the SQLite transaction is rolled back without touching catalogue data.
    pub fn rebase_installed_pack_paths(
        &mut self,
        legacy_app_root: &Path,
        current_app_root: &Path,
    ) -> Result<usize, PersistenceError> {
        if !legacy_app_root.is_absolute()
            || !current_app_root.is_absolute()
            || legacy_app_root == current_app_root
        {
            return Err(PersistenceError::Storage(
                "install-path migration roots are invalid".into(),
            ));
        }
        ensure_plain_existing_directory(current_app_root)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = transaction
            .prepare("SELECT pack_id,install_path FROM installed_packs ORDER BY pack_id")?;
        let records = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut updates = Vec::new();
        for (pack_id, stored) in records {
            if !valid_identifier(&pack_id) {
                return Err(PersistenceError::Storage(
                    "installed pack has an invalid identifier".into(),
                ));
            }
            let stored = PathBuf::from(stored);
            if !stored.is_absolute() {
                return Err(PersistenceError::Storage(format!(
                    "installed pack {pack_id} has a relative path"
                )));
            }
            let (suffix, needs_update) = if let Ok(suffix) = stored.strip_prefix(current_app_root) {
                (suffix.to_path_buf(), false)
            } else if let Ok(suffix) = stored.strip_prefix(legacy_app_root) {
                (suffix.to_path_buf(), true)
            } else {
                return Err(PersistenceError::Storage(format!(
                    "installed pack {pack_id} points outside the application data roots"
                )));
            };
            validate_install_suffix(&suffix, &pack_id)?;
            let rebased = current_app_root.join(&suffix);
            ensure_plain_existing_directory(&rebased)?;
            if needs_update {
                updates.push((pack_id, rebased.to_string_lossy().into_owned()));
            }
        }
        for (pack_id, rebased) in &updates {
            let changed = transaction.execute(
                "UPDATE installed_packs SET install_path=?2 WHERE pack_id=?1",
                rusqlite::params![pack_id, rebased],
            )?;
            if changed != 1 {
                return Err(PersistenceError::Storage(
                    "installed pack changed during path migration".into(),
                ));
            }
        }
        transaction.commit()?;
        Ok(updates.len())
    }

    #[cfg(test)]
    fn schema_version(&self) -> Result<i64, PersistenceError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }
}

fn validate_install_suffix(suffix: &Path, pack_id: &str) -> Result<(), PersistenceError> {
    let components = suffix.components().collect::<Vec<_>>();
    let valid = matches!(components.as_slice(),
        [Component::Normal(content), Component::Normal(packs), Component::Normal(pack), Component::Normal(version)]
            if *content == "content"
                && *packs == "packs"
                && *pack == pack_id
                && version.to_string_lossy().len() == 64
                && version.to_string_lossy().bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );
    if valid {
        Ok(())
    } else {
        Err(PersistenceError::Storage(format!(
            "installed pack {pack_id} has an unsafe service path"
        )))
    }
}

fn ensure_plain_existing_directory(path: &Path) -> Result<(), PersistenceError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        PersistenceError::Storage(format!(
            "installed pack path {} is unavailable: {error}",
            path.display()
        ))
    })?;
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        linked || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    };
    if linked || !metadata.is_dir() {
        return Err(PersistenceError::Storage(format!(
            "installed pack path {} is linked or not a directory",
            path.display()
        )));
    }
    Ok(())
}

impl StudioJobStore for PreferencesRepository {
    fn create_studio_job(&mut self, job: &StudioJobRecord) -> Result<(), PersistenceError> {
        let job = validate_studio_job(job)?;
        if job.state != StudioJobState::Queued
            || job.revision != 0
            || job.created_at_ms != job.updated_at_ms
            || job.failure.is_some()
        {
            return Err(PersistenceError::InvalidStudioJobUpdate);
        }
        let request_json = studio_json(&job.request)?;
        let prompt_json = studio_json(&job.prompt)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM music_studio_jobs WHERE job_id = ?1)",
            [job.job_id.as_str()],
            |row| row.get(0),
        )?;
        if exists {
            return Err(PersistenceError::DuplicateStudioJob(
                job.job_id.as_str().to_owned(),
            ));
        }
        let attempt_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM music_studio_jobs WHERE attempt_id = ?1)",
            [job.attempt_id.as_str()],
            |row| row.get(0),
        )?;
        if attempt_exists {
            return Err(PersistenceError::DuplicateStudioAttempt(
                job.attempt_id.as_str().to_owned(),
            ));
        }
        tx.execute(
            "INSERT INTO music_studio_jobs(job_id, attempt_id, request_json, prompt_json, state, revision, created_at_ms, updated_at_ms, failure_json) VALUES(?1, ?2, ?3, ?4, 'queued', 0, ?5, ?5, NULL)",
            rusqlite::params![job.job_id.as_str(), job.attempt_id.as_str(), request_json, prompt_json, studio_timestamp(job.created_at_ms)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn load_studio_job(
        &mut self,
        job_id: &StudioJobId,
    ) -> Result<Option<StudioJobRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT job_id, attempt_id, request_json, prompt_json, state, revision, created_at_ms, updated_at_ms, failure_json FROM music_studio_jobs WHERE job_id = ?1",
        )?;
        let row = statement
            .query_row([job_id.as_str()], studio_job_row)
            .optional()?;
        row.map(decode_studio_job).transpose()
    }

    fn recent_studio_jobs(
        &mut self,
        limit: usize,
    ) -> Result<Vec<StudioJobRecord>, PersistenceError> {
        if !(1..=MAX_RECENT_STUDIO_JOBS).contains(&limit) {
            return Err(PersistenceError::InvalidStudioJobUpdate);
        }
        let limit = i64::try_from(limit).map_err(|_| PersistenceError::InvalidStudioJobUpdate)?;
        let mut statement = self.connection.prepare(
            "SELECT job_id, attempt_id, request_json, prompt_json, state, revision, created_at_ms, updated_at_ms, failure_json FROM music_studio_jobs ORDER BY updated_at_ms DESC, created_at_ms DESC, job_id DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map([limit], studio_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode_studio_job).collect()
    }

    fn transition_studio_job(
        &mut self,
        job_id: &StudioJobId,
        expected_revision: u64,
        next: StudioJobState,
        updated_at_ms: u64,
        failure: Option<StudioFailureDetails>,
    ) -> Result<StudioJobRecord, PersistenceError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_studio_job_from_transaction(&tx, job_id)?
            .ok_or_else(|| PersistenceError::UnknownStudioJob(job_id.as_str().to_owned()))?;
        if current.revision != expected_revision {
            return Err(PersistenceError::StaleStudioJob(job_id.as_str().to_owned()));
        }
        let mut updated = current.clone();
        updated
            .transition(expected_revision, next, updated_at_ms, failure)
            .map_err(|error| match error.code {
                StudioErrorCode::StaleRevision => {
                    PersistenceError::StaleStudioJob(job_id.as_str().to_owned())
                }
                _ => PersistenceError::InvalidStudioJobUpdate,
            })?;
        let changed = tx.execute(
            "UPDATE music_studio_jobs SET state = ?2, revision = ?3, updated_at_ms = ?4, failure_json = ?5 WHERE job_id = ?1 AND revision = ?6",
            rusqlite::params![
                job_id.as_str(),
                studio_state_key(updated.state),
                studio_revision(updated.revision)?,
                studio_timestamp(updated.updated_at_ms)?,
                updated.failure.as_ref().map(studio_json).transpose()?,
                studio_revision(expected_revision)?,
            ],
        )?;
        if changed != 1 {
            return Err(PersistenceError::StaleStudioJob(job_id.as_str().to_owned()));
        }
        tx.commit()?;
        Ok(updated)
    }

    fn recover_studio_jobs(&mut self, recovered_at_ms: u64) -> Result<(), PersistenceError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare(
            "SELECT job_id, attempt_id, request_json, prompt_json, state, revision, created_at_ms, updated_at_ms, failure_json FROM music_studio_jobs WHERE state IN ('queued', 'generating', 'analyzing', 'saving')",
        )?;
        let jobs = statement
            .query_map([], studio_job_row)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(decode_studio_job)
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for mut job in jobs {
            let next = if job.state == StudioJobState::Saving {
                // Saving is an exactly-retryable operation. A durable install receipt may
                // still need reconciliation, or the generated pack may already have been
                // committed before the process stopped. Returning to Ready lets the save
                // path reconcile and compare the deterministic registration instead of
                // stranding valid work in a terminal failure state.
                StudioJobState::Ready
            } else {
                StudioJobState::Interrupted
            };
            let failure = (next == StudioJobState::Failed)
                .then(|| {
                    StudioFailureDetails::new(
                        StudioErrorCode::InvalidRequest,
                        "music studio work was interrupted".to_owned(),
                    )
                    .map_err(|_| PersistenceError::InvalidStudioJobUpdate)
                })
                .transpose()?;
            let updated_at_ms = recovered_at_ms.max(job.updated_at_ms);
            let revision = job.revision;
            job.transition(revision, next, updated_at_ms, failure)
                .map_err(|_| PersistenceError::InvalidStudioJobUpdate)?;
            tx.execute(
                "UPDATE music_studio_jobs SET state = ?2, revision = ?3, updated_at_ms = ?4, failure_json = ?5 WHERE job_id = ?1 AND revision = ?6",
                rusqlite::params![
                    job.job_id.as_str(), studio_state_key(job.state), studio_revision(job.revision)?,
                    studio_timestamp(job.updated_at_ms)?, job.failure.as_ref().map(studio_json).transpose()?,
                    studio_revision(revision)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn upsert_studio_job_artifact(
        &mut self,
        artifact: &StudioJobArtifact,
    ) -> Result<(), PersistenceError> {
        validate_studio_artifact(artifact)?;
        self.connection.execute(
            "INSERT INTO music_studio_job_artifacts(job_id,parent_job_id,runtime_version,stage,output_relative_path,output_sha256,analysis_json,safe_error_code,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(job_id) DO UPDATE SET runtime_version=excluded.runtime_version,stage=excluded.stage,output_relative_path=excluded.output_relative_path,output_sha256=excluded.output_sha256,analysis_json=excluded.analysis_json,safe_error_code=excluded.safe_error_code,updated_at_ms=excluded.updated_at_ms",
            rusqlite::params![artifact.job_id.as_str(), artifact.parent_job_id.as_ref().map(StudioJobId::as_str), artifact.runtime_version, artifact.stage, artifact.output_relative_path, artifact.output_sha256, artifact.analysis_json, artifact.safe_error_code, studio_timestamp(artifact.created_at_ms)?, studio_timestamp(artifact.updated_at_ms)?],
        )?;
        Ok(())
    }

    fn load_studio_job_artifact(
        &mut self,
        job_id: &StudioJobId,
    ) -> Result<Option<StudioJobArtifact>, PersistenceError> {
        self.connection.query_row("SELECT job_id,parent_job_id,runtime_version,stage,output_relative_path,output_sha256,analysis_json,safe_error_code,created_at_ms,updated_at_ms FROM music_studio_job_artifacts WHERE job_id=?1", [job_id.as_str()], |r| {
            Ok(StudioJobArtifact { job_id: StudioJobId::new(r.get::<_,String>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?, parent_job_id: r.get::<_,Option<String>>(1)?.map(StudioJobId::new).transpose().map_err(|_| rusqlite::Error::InvalidQuery)?, runtime_version:r.get(2)?, stage:r.get(3)?, output_relative_path:r.get(4)?, output_sha256:r.get(5)?, analysis_json:r.get(6)?, safe_error_code:r.get(7)?, created_at_ms:u64::try_from(r.get::<_,i64>(8)?).map_err(|_|rusqlite::Error::InvalidQuery)?, updated_at_ms:u64::try_from(r.get::<_,i64>(9)?).map_err(|_|rusqlite::Error::InvalidQuery)? })
        }).optional().map_err(Into::into).and_then(|x| { if let Some(ref a)=x { validate_studio_artifact(a)?; }; Ok(x) })
    }

    fn remove_studio_job_artifact(&mut self, job_id: &StudioJobId) -> Result<(), PersistenceError> {
        self.connection.execute(
            "DELETE FROM music_studio_job_artifacts WHERE job_id=?1",
            [job_id.as_str()],
        )?;
        Ok(())
    }

    fn remove_studio_job(&mut self, job_id: &StudioJobId) -> Result<(), PersistenceError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE music_studio_job_artifacts SET parent_job_id=NULL WHERE parent_job_id=?1",
            [job_id.as_str()],
        )?;
        let changed = transaction.execute(
            "DELETE FROM music_studio_jobs WHERE job_id=?1",
            [job_id.as_str()],
        )?;
        if changed != 1 {
            return Err(PersistenceError::UnknownStudioJob(
                job_id.as_str().to_owned(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }
}

fn validate_studio_artifact(value: &StudioJobArtifact) -> Result<(), PersistenceError> {
    let safe = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    };
    const STAGES: &[&str] = &[
        "queued",
        "generating",
        "analyzing",
        "ready",
        "rejected",
        "failed",
        "cancelled",
        "interrupted",
    ];
    const ERROR_CODES: &[&str] = &[
        "runtime_invalid",
        "spawn_failed",
        "timeout",
        "gpu_oom",
        "unexpected_exit",
        "output_invalid",
        "analysis_rejected",
        "interrupted",
    ];
    if !safe(&value.runtime_version, 80)
        || !STAGES.contains(&value.stage.as_str())
        || value.updated_at_ms < value.created_at_ms
        || value.output_relative_path.as_ref().is_some_and(|p| {
            p.len() > 240
                || p.starts_with('/')
                || Path::new(p).is_absolute()
                || p.contains('\\')
                || p.split('/').any(|x| x.is_empty() || x == "." || x == "..")
        })
        || value.output_sha256.as_ref().is_some_and(|h| {
            h.len() != 64
                || !h
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        || value.analysis_json.as_ref().is_some_and(|j| {
            j.len() > 16 * 1024 || serde_json::from_str::<serde_json::Value>(j).is_err()
        })
        || value
            .safe_error_code
            .as_ref()
            .is_some_and(|c| !ERROR_CODES.contains(&c.as_str()))
        || (value.stage == "ready"
            && (value.output_relative_path.is_none()
                || value.output_sha256.is_none()
                || value.analysis_json.is_none()
                || value.safe_error_code.is_some()))
    {
        return Err(PersistenceError::InvalidStudioJob);
    }
    Ok(())
}

type StudioJobRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
);

fn studio_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StudioJobRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, i64>(5)?,
        row.get::<_, i64>(6)?,
        row.get::<_, i64>(7)?,
        row.get::<_, Option<String>>(8)?,
    ))
}

fn load_studio_job_from_transaction(
    transaction: &rusqlite::Transaction<'_>,
    job_id: &StudioJobId,
) -> Result<Option<StudioJobRecord>, PersistenceError> {
    let row = transaction.query_row(
        "SELECT job_id, attempt_id, request_json, prompt_json, state, revision, created_at_ms, updated_at_ms, failure_json FROM music_studio_jobs WHERE job_id = ?1",
        [job_id.as_str()],
        studio_job_row,
    ).optional()?;
    row.map(decode_studio_job).transpose()
}

fn decode_studio_job(values: StudioJobRow) -> Result<StudioJobRecord, PersistenceError> {
    let (
        job_id,
        attempt_id,
        request,
        prompt,
        state,
        revision,
        created_at_ms,
        updated_at_ms,
        failure,
    ) = values;
    let request: serde_json::Value =
        serde_json::from_str(&request).map_err(|_| PersistenceError::InvalidStudioJob)?;
    let prompt: serde_json::Value =
        serde_json::from_str(&prompt).map_err(|_| PersistenceError::InvalidStudioJob)?;
    let failure = failure
        .map(|value| {
            serde_json::from_str::<serde_json::Value>(&value)
                .map_err(|_| PersistenceError::InvalidStudioJob)
        })
        .transpose()?;
    serde_json::from_value(serde_json::json!({
        "job_id": job_id, "attempt_id": attempt_id, "request": request, "prompt": prompt,
        "state": state, "revision": u64::try_from(revision).map_err(|_| PersistenceError::InvalidStudioJob)?,
        "created_at_ms": u64::try_from(created_at_ms).map_err(|_| PersistenceError::InvalidStudioJob)?,
        "updated_at_ms": u64::try_from(updated_at_ms).map_err(|_| PersistenceError::InvalidStudioJob)?,
        "failure": failure,
    })).map_err(|_| PersistenceError::InvalidStudioJob)
}

fn validate_studio_job(job: &StudioJobRecord) -> Result<StudioJobRecord, PersistenceError> {
    serde_json::from_value(
        serde_json::to_value(job).map_err(|_| PersistenceError::InvalidStudioJob)?,
    )
    .map_err(|_| PersistenceError::InvalidStudioJob)
}

fn studio_json<T: Serialize>(value: &T) -> Result<String, PersistenceError> {
    serde_json::to_string(value).map_err(|_| PersistenceError::InvalidStudioJob)
}

fn studio_timestamp(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::InvalidStudioJobUpdate)
}

fn studio_revision(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::InvalidStudioJobUpdate)
}

fn studio_state_key(state: StudioJobState) -> &'static str {
    match state {
        StudioJobState::Queued => "queued",
        StudioJobState::Generating => "generating",
        StudioJobState::Analyzing => "analyzing",
        StudioJobState::Ready => "ready",
        StudioJobState::Rejected => "rejected",
        StudioJobState::Failed => "failed",
        StudioJobState::Cancelled => "cancelled",
        StudioJobState::Interrupted => "interrupted",
        StudioJobState::Saving => "saving",
        StudioJobState::Saved => "saved",
    }
}

impl SessionHistoryStore for PreferencesRepository {
    fn complete_onboarding_with_history(
        &mut self,
        intensity: Intensity,
        genres: &[String],
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError> {
        validate_onboarding(intensity, genres)?;
        let mut ordered = genres.to_vec();
        ordered.sort();
        let session_type = SessionType::Countdown { seconds: 1_800 };
        let config = serde_json::to_string(&session_type)
            .map_err(|error| PersistenceError::Storage(error.to_string()))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM onboarding_genres", [])?;
        for (position, genre) in ordered.iter().enumerate() {
            tx.execute(
                "INSERT INTO onboarding_genres(genre_id, position) VALUES(?1, ?2)",
                rusqlite::params![genre, position],
            )?;
        }
        tx.execute("INSERT INTO onboarding_preferences(singleton, completed, intensity) VALUES(1, 1, ?1) ON CONFLICT(singleton) DO UPDATE SET completed=1, intensity=excluded.intensity", [intensity.storage_key()])?;
        tx.execute("INSERT INTO application_preferences(singleton,last_activity) VALUES(1,'deep_work') ON CONFLICT(singleton) DO UPDATE SET last_activity='deep_work'", [])?;
        tx.execute("INSERT INTO activity_preferences(activity,intensity) VALUES('deep_work',?1) ON CONFLICT(activity) DO UPDATE SET intensity=excluded.intensity", [intensity.storage_key()])?;
        tx.execute("INSERT INTO activity_timer_preferences(activity,timer_kind,countdown_seconds,work_seconds,break_seconds,repeats) VALUES('deep_work','countdown',1800,NULL,NULL,NULL) ON CONFLICT(activity) DO UPDATE SET timer_kind='countdown',countdown_seconds=1800,work_seconds=NULL,break_seconds=NULL,repeats=NULL", [])?;
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute("INSERT INTO session_history(id, activity, intensity, session_type, started_at) VALUES(?1, 'deep_work', ?2, ?3, ?4)", rusqlite::params![id, intensity.storage_key(), config, i64::try_from(started_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?])?;
        prune_history(&tx)?;
        tx.commit()?;
        Ok(SessionHistoryRecord {
            id,
            activity: Activity::DeepWork,
            intensity,
            session_type,
            started_at,
            ended_at: None,
            end_reason: None,
            focus_seconds: None,
            focus_outcome: None,
            sound_enjoyment: None,
        })
    }

    fn save_last_activity_with_history(
        &mut self,
        activity: Activity,
        intensity: Intensity,
        session_type: SessionType,
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError> {
        let config = serde_json::to_string(&session_type)
            .map_err(|error| PersistenceError::Storage(error.to_string()))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO application_preferences(singleton, last_activity) VALUES(1, ?1) ON CONFLICT(singleton) DO UPDATE SET last_activity = excluded.last_activity", [activity.storage_key()])?;
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute("INSERT INTO session_history(id, activity, intensity, session_type, started_at) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, activity.storage_key(), intensity.storage_key(), config, i64::try_from(started_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?])?;
        prune_history(&tx)?;
        tx.commit()?;
        Ok(SessionHistoryRecord {
            id,
            activity,
            intensity,
            session_type,
            started_at,
            ended_at: None,
            end_reason: None,
            focus_seconds: None,
            focus_outcome: None,
            sound_enjoyment: None,
        })
    }
    fn begin_session_history(
        &mut self,
        activity: Activity,
        intensity: Intensity,
        session_type: SessionType,
        started_at: u64,
    ) -> Result<SessionHistoryRecord, PersistenceError> {
        let config = serde_json::to_string(&session_type)
            .map_err(|error| PersistenceError::Storage(error.to_string()))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // A stale active row means the process was interrupted.  It remains
        // deliberately duration-less because a monotonic clock cannot survive restart.
        tx.execute("UPDATE session_history SET ended_at = CASE WHEN started_at > ?1 THEN started_at ELSE ?1 END, end_reason = 'interrupted', focus_seconds = NULL WHERE ended_at IS NULL", [i64::try_from(started_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?])?;
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute("INSERT INTO session_history(id, activity, intensity, session_type, started_at) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, activity.storage_key(), intensity.storage_key(), config, i64::try_from(started_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?])?;
        // Deterministic retention: active row is never part of the limit.
        prune_history(&tx)?;
        tx.commit()?;
        Ok(SessionHistoryRecord {
            id,
            activity,
            intensity,
            session_type,
            started_at,
            ended_at: None,
            end_reason: None,
            focus_seconds: None,
            focus_outcome: None,
            sound_enjoyment: None,
        })
    }

    fn finalize_session_history(
        &mut self,
        id: &str,
        reason: SessionEndReason,
        ended_at: u64,
        focus_seconds: u64,
    ) -> Result<(), PersistenceError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute("UPDATE session_history SET ended_at = ?2, end_reason = ?3, focus_seconds = ?4 WHERE id = ?1 AND ended_at IS NULL", rusqlite::params![id, i64::try_from(ended_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?, reason.key(), i64::try_from(focus_seconds).map_err(|_| PersistenceError::Storage("focus duration overflow".into()))?])?;
        if changed != 1 {
            return Err(PersistenceError::UnknownSessionHistory(id.to_owned()));
        }
        prune_history(&tx)?;
        tx.commit()?;
        Ok(())
    }

    fn reconcile_interrupted_session_history(
        &mut self,
        ended_at: u64,
    ) -> Result<(), PersistenceError> {
        self.connection.execute("UPDATE session_history SET ended_at = CASE WHEN started_at > ?1 THEN started_at ELSE ?1 END, end_reason = 'interrupted', focus_seconds = NULL WHERE ended_at IS NULL", [i64::try_from(ended_at).map_err(|_| PersistenceError::Storage("timestamp overflow".into()))?])?;
        Ok(())
    }

    fn recent_session_history(
        &mut self,
        limit: usize,
    ) -> Result<Vec<SessionHistoryRecord>, PersistenceError> {
        let mut statement = self.connection.prepare("SELECT id, activity, intensity, session_type, started_at, ended_at, end_reason, focus_seconds, focus_outcome, sound_enjoyment FROM session_history WHERE ended_at IS NOT NULL ORDER BY ended_at DESC, started_at DESC, id DESC LIMIT ?1")?;
        let records = statement
            .query_map([i64::try_from(limit.min(100)).unwrap_or(100)], history_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    fn save_session_ratings(
        &mut self,
        id: &str,
        focus_outcome: Option<SessionFocusOutcome>,
        sound_enjoyment: Option<SessionSoundEnjoyment>,
    ) -> Result<(), PersistenceError> {
        let changed = self.connection.execute("UPDATE session_history SET focus_outcome = ?2, sound_enjoyment = ?3 WHERE id = ?1 AND ended_at IS NOT NULL", rusqlite::params![id, focus_outcome.map(SessionFocusOutcome::key), sound_enjoyment.map(SessionSoundEnjoyment::key)])?;
        if changed != 1 {
            return Err(PersistenceError::UnknownSessionHistory(id.to_owned()));
        }
        Ok(())
    }
}

fn prune_history(tx: &rusqlite::Transaction<'_>) -> Result<(), PersistenceError> {
    tx.execute("DELETE FROM session_history WHERE ended_at IS NOT NULL AND id NOT IN (SELECT id FROM session_history WHERE ended_at IS NOT NULL ORDER BY ended_at DESC, started_at DESC, id DESC LIMIT 100)", [])?;
    Ok(())
}

fn history_row(row: &rusqlite::Row<'_>) -> Result<SessionHistoryRecord, rusqlite::Error> {
    let invalid = |message: String| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        )
    };
    let activity: String = row.get(1)?;
    let intensity: String = row.get(2)?;
    let config: String = row.get(3)?;
    let started: i64 = row.get(4)?;
    let ended: Option<i64> = row.get(5)?;
    let focus: Option<i64> = row.get(7)?;
    let id: String = row.get(0)?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid("id".into()));
    }
    let activity =
        Activity::from_storage_key(&activity).ok_or_else(|| invalid("activity".into()))?;
    let intensity = Intensity::from_storage_key(&intensity)
        .filter(|value| *value != Intensity::Off)
        .ok_or_else(|| invalid("intensity".into()))?;
    let session_type: SessionType =
        serde_json::from_str(&config).map_err(|_| invalid("session type".into()))?;
    session_type
        .validate()
        .map_err(|_| invalid("session type".into()))?;
    let started_at = u64::try_from(started).map_err(|_| invalid("started_at".into()))?;
    let ended_at = ended
        .map(|value| u64::try_from(value).map_err(|_| invalid("ended_at".into())))
        .transpose()?;
    let end_reason = row
        .get::<_, Option<String>>(6)?
        .map(|value| SessionEndReason::parse(&value).ok_or_else(|| invalid("end reason".into())))
        .transpose()?;
    let focus_seconds = focus
        .map(|value| u64::try_from(value).map_err(|_| invalid("focus".into())))
        .transpose()?;
    let focus_outcome = row
        .get::<_, Option<String>>(8)?
        .map(|value| {
            SessionFocusOutcome::parse(&value).ok_or_else(|| invalid("focus outcome".into()))
        })
        .transpose()?;
    let sound_enjoyment = row
        .get::<_, Option<String>>(9)?
        .map(|value| {
            SessionSoundEnjoyment::parse(&value).ok_or_else(|| invalid("sound enjoyment".into()))
        })
        .transpose()?;
    match (ended_at, end_reason, focus_seconds) {
        (None, None, None) if focus_outcome.is_none() && sound_enjoyment.is_none() => {}
        (Some(end), Some(SessionEndReason::Interrupted), None) if end >= started_at => {}
        (Some(end), Some(_), Some(_)) if end >= started_at => {}
        _ => return Err(invalid("history lifecycle".into())),
    }
    Ok(SessionHistoryRecord {
        id,
        activity,
        intensity,
        session_type,
        started_at,
        ended_at,
        end_reason,
        focus_seconds,
        focus_outcome,
        sound_enjoyment,
    })
}

impl PreferenceStore for PreferencesRepository {
    fn load_last_activity(&mut self) -> Result<Option<Activity>, PersistenceError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT last_activity FROM application_preferences WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| {
                Activity::from_storage_key(&value).ok_or(PersistenceError::InvalidActivity(value))
            })
            .transpose()
    }

    fn save_last_activity(&mut self, activity: Activity) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO application_preferences(singleton, last_activity) VALUES(1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET last_activity = excluded.last_activity",
            [activity.storage_key()],
        )?;
        Ok(())
    }

    fn load_intensity(
        &mut self,
        activity: Activity,
    ) -> Result<Option<Intensity>, PersistenceError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT intensity FROM activity_preferences WHERE activity = ?1",
                [activity.storage_key()],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| {
                Intensity::from_storage_key(&value).ok_or(PersistenceError::InvalidIntensity(value))
            })
            .transpose()
    }

    fn save_intensity(
        &mut self,
        activity: Activity,
        intensity: Intensity,
    ) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO activity_preferences(activity, intensity) VALUES(?1, ?2)
             ON CONFLICT(activity) DO UPDATE SET intensity = excluded.intensity",
            [activity.storage_key(), intensity.storage_key()],
        )?;
        Ok(())
    }

    fn load_master_volume(&mut self) -> Result<Option<MasterVolume>, PersistenceError> {
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT master_volume FROM application_preferences WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| {
                u8::try_from(value)
                    .ok()
                    .and_then(|v| MasterVolume::new(v).ok())
                    .ok_or(PersistenceError::InvalidMasterVolume(value))
            })
            .transpose()
    }

    fn save_master_volume(&mut self, volume: MasterVolume) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO application_preferences(singleton, last_activity, master_volume) VALUES(1, 'deep_work', ?1)
             ON CONFLICT(singleton) DO UPDATE SET master_volume = excluded.master_volume",
            [i64::from(volume.percent())],
        )?;
        Ok(())
    }

    fn load_session_type(
        &mut self,
        activity: Activity,
    ) -> Result<Option<SessionType>, PersistenceError> {
        type TimerRow = (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>);
        let row: Option<TimerRow> = self
            .connection
            .query_row(
                "SELECT timer_kind, countdown_seconds, work_seconds, break_seconds, repeats
                 FROM activity_timer_preferences WHERE activity = ?1",
                [activity.storage_key()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((kind, countdown, work, rest, repeats)) = row else {
            return Ok(None);
        };
        let invalid = || PersistenceError::InvalidTimerConfig {
            activity: activity.storage_key().to_owned(),
            reason: format!("invalid {kind} timer columns"),
        };
        let timer = match kind.as_str() {
            "infinite"
                if countdown.is_none() && work.is_none() && rest.is_none() && repeats.is_none() =>
            {
                SessionType::Infinite
            }
            "countdown" if work.is_none() && rest.is_none() && repeats.is_none() => {
                SessionType::Countdown {
                    seconds: u64::try_from(countdown.ok_or_else(invalid)?)
                        .map_err(|_| invalid())?,
                }
            }
            "interval" if countdown.is_none() => SessionType::Interval {
                work_seconds: u64::try_from(work.ok_or_else(invalid)?).map_err(|_| invalid())?,
                break_seconds: u64::try_from(rest.ok_or_else(invalid)?).map_err(|_| invalid())?,
                repeats: u32::try_from(repeats.ok_or_else(invalid)?).map_err(|_| invalid())?,
            },
            _ => return Err(invalid()),
        };
        timer
            .validate()
            .map_err(|error| PersistenceError::InvalidTimerConfig {
                activity: activity.storage_key().to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Some(timer))
    }

    fn save_session_type(
        &mut self,
        activity: Activity,
        session_type: SessionType,
    ) -> Result<(), PersistenceError> {
        session_type
            .validate()
            .map_err(|error| PersistenceError::InvalidTimerConfig {
                activity: activity.storage_key().to_owned(),
                reason: error.to_string(),
            })?;
        let (kind, countdown, work, rest, repeats): (
            &str,
            Option<u64>,
            Option<u64>,
            Option<u64>,
            Option<u32>,
        ) = match session_type {
            SessionType::Infinite => ("infinite", None, None, None, None),
            SessionType::Countdown { seconds } => ("countdown", Some(seconds), None, None, None),
            SessionType::Interval {
                work_seconds,
                break_seconds,
                repeats,
            } => (
                "interval",
                None,
                Some(work_seconds),
                Some(break_seconds),
                Some(repeats),
            ),
        };
        self.connection.execute(
            "INSERT INTO activity_timer_preferences(
                activity, timer_kind, countdown_seconds, work_seconds, break_seconds, repeats
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(activity) DO UPDATE SET
                timer_kind = excluded.timer_kind,
                countdown_seconds = excluded.countdown_seconds,
                work_seconds = excluded.work_seconds,
                break_seconds = excluded.break_seconds,
                repeats = excluded.repeats",
            rusqlite::params![activity.storage_key(), kind, countdown, work, rest, repeats],
        )?;
        Ok(())
    }
}

impl OnboardingStore for PreferencesRepository {
    fn onboarding_preferences(&mut self) -> Result<OnboardingPreferences, PersistenceError> {
        let (completed, intensity): (i64, String) = self.connection.query_row(
            "SELECT completed, intensity FROM onboarding_preferences WHERE singleton = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let stored_intensity = intensity;
        let intensity = Intensity::from_storage_key(&stored_intensity)
            .filter(|value| matches!(value, Intensity::Low | Intensity::Medium | Intensity::High))
            .ok_or(PersistenceError::InvalidIntensity(stored_intensity))?;
        let mut statement = self
            .connection
            .prepare("SELECT genre_id FROM onboarding_genres ORDER BY position ASC")?;
        let genres = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if genres.len() > 3 {
            return Err(PersistenceError::TooManyOnboardingGenres);
        }
        for genre in &genres {
            if !ONBOARDING_GENRE_IDS.contains(&genre.as_str()) {
                return Err(PersistenceError::InvalidGenreId(genre.clone()));
            }
        }
        Ok(OnboardingPreferences {
            completed: completed == 1,
            intensity,
            genres,
        })
    }

    fn complete_onboarding(
        &mut self,
        intensity: Intensity,
        genres: &[String],
    ) -> Result<(), PersistenceError> {
        validate_onboarding(intensity, genres)?;
        let mut ordered = genres.to_vec();
        ordered.sort();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM onboarding_genres", [])?;
        for (position, genre) in ordered.iter().enumerate() {
            tx.execute(
                "INSERT INTO onboarding_genres(genre_id, position) VALUES(?1, ?2)",
                rusqlite::params![genre, position],
            )?;
        }
        tx.execute("INSERT INTO onboarding_preferences(singleton, completed, intensity) VALUES(1, 1, ?1) ON CONFLICT(singleton) DO UPDATE SET completed=1, intensity=excluded.intensity", [intensity.storage_key()])?;
        tx.execute("INSERT INTO application_preferences(singleton,last_activity) VALUES(1,'deep_work') ON CONFLICT(singleton) DO UPDATE SET last_activity='deep_work'", [])?;
        tx.execute("INSERT INTO activity_preferences(activity,intensity) VALUES('deep_work',?1) ON CONFLICT(activity) DO UPDATE SET intensity=excluded.intensity", [intensity.storage_key()])?;
        tx.execute("INSERT INTO activity_timer_preferences(activity,timer_kind,countdown_seconds,work_seconds,break_seconds,repeats) VALUES('deep_work','countdown',1800,NULL,NULL,NULL) ON CONFLICT(activity) DO UPDATE SET timer_kind='countdown',countdown_seconds=1800,work_seconds=NULL,break_seconds=NULL,repeats=NULL", [])?;
        tx.commit()?;
        Ok(())
    }
}

fn validate_onboarding(intensity: Intensity, genres: &[String]) -> Result<(), PersistenceError> {
    if !matches!(
        intensity,
        Intensity::Low | Intensity::Medium | Intensity::High
    ) {
        return Err(PersistenceError::InvalidIntensity(
            intensity.storage_key().to_owned(),
        ));
    }
    if genres.len() > 3 {
        return Err(PersistenceError::TooManyOnboardingGenres);
    }
    let mut ordered = genres.to_vec();
    ordered.sort();
    ordered.dedup();
    if ordered.len() != genres.len() {
        return Err(PersistenceError::TooManyOnboardingGenres);
    }
    for genre in &ordered {
        if !ONBOARDING_GENRE_IDS.contains(&genre.as_str()) {
            return Err(PersistenceError::InvalidGenreId(genre.clone()));
        }
    }
    Ok(())
}

impl GenrePreferenceStore for PreferencesRepository {
    fn load_genre_preference(
        &mut self,
        activity: Activity,
    ) -> Result<Option<String>, PersistenceError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT genre_id FROM activity_genre_preferences WHERE activity = ?1",
                [activity.storage_key()],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|genre_id| {
                if valid_identifier(&genre_id) {
                    Ok(genre_id)
                } else {
                    Err(PersistenceError::InvalidGenreId(genre_id))
                }
            })
            .transpose()
    }

    fn save_genre_preference(
        &mut self,
        activity: Activity,
        genre_id: &str,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(genre_id) {
            return Err(PersistenceError::InvalidGenreId(genre_id.to_owned()));
        }
        self.connection.execute(
            "INSERT INTO activity_genre_preferences(activity, genre_id) VALUES(?1, ?2)
             ON CONFLICT(activity) DO UPDATE SET genre_id = excluded.genre_id",
            [activity.storage_key(), genre_id],
        )?;
        Ok(())
    }

    fn clear_genre_preference(&mut self, activity: Activity) -> Result<(), PersistenceError> {
        self.connection.execute(
            "DELETE FROM activity_genre_preferences WHERE activity = ?1",
            [activity.storage_key()],
        )?;
        Ok(())
    }
}

impl MoodPreferenceStore for PreferencesRepository {
    fn load_mood_preference(
        &mut self,
        activity: Activity,
    ) -> Result<Option<String>, PersistenceError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT mood_id FROM activity_mood_preferences WHERE activity = ?1",
                [activity.storage_key()],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|mood_id| {
                if valid_identifier(&mood_id) {
                    Ok(mood_id)
                } else {
                    Err(PersistenceError::InvalidMoodId(mood_id))
                }
            })
            .transpose()
    }
    fn save_mood_preference(
        &mut self,
        activity: Activity,
        mood_id: &str,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(mood_id) {
            return Err(PersistenceError::InvalidMoodId(mood_id.to_owned()));
        }
        self.connection.execute("INSERT INTO activity_mood_preferences(activity, mood_id) VALUES(?1, ?2) ON CONFLICT(activity) DO UPDATE SET mood_id = excluded.mood_id", [activity.storage_key(), mood_id])?;
        Ok(())
    }
    fn clear_mood_preference(&mut self, activity: Activity) -> Result<(), PersistenceError> {
        self.connection.execute(
            "DELETE FROM activity_mood_preferences WHERE activity = ?1",
            [activity.storage_key()],
        )?;
        Ok(())
    }
}

fn valid_identifier(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(byte, b'a'..=b'z' | b'0'..=b'9')
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGeneratedLocalEvidence {
    producer: String,
    job_id: String,
    completed_at_unix_seconds: i64,
}

const GENERATED_LOCAL_EVIDENCE_PRODUCER: &str = "adhd-music-studio";

fn validate_generated_local_customer(
    registration: &PackRegistration,
    customer: &GeneratedLocalCustomerRecord,
) -> Result<(), PersistenceError> {
    validate_customer_title(&customer.title)?;
    let pack = &registration.pack;
    let job_id = pack
        .pack_id
        .strip_prefix("generated.local.")
        .filter(|value| valid_identifier(value))
        .ok_or_else(|| PersistenceError::Storage("invalid generated-local pack id".into()))?;
    let expected_item = format!("{}.item", pack.pack_id);
    let evidence = registration
        .generated_local_evidence
        .as_ref()
        .ok_or_else(|| PersistenceError::Storage("generated-local evidence is missing".into()))?;
    let parsed: StoredGeneratedLocalEvidence = serde_json::from_str(&evidence.evidence_json)
        .map_err(|_| PersistenceError::Storage("generated-local evidence is invalid".into()))?;
    if pack.status != "generated_local"
        || !valid_identifier(&pack.pack_id)
        || !valid_identifier(&expected_item)
        || registration.items.len() != 1
        || registration.items[0].item_id != expected_item
        || customer.pack_id != pack.pack_id
        || customer.item_id != expected_item
        || evidence.generation_job_id != job_id
        || parsed.producer != GENERATED_LOCAL_EVIDENCE_PRODUCER
        || parsed.job_id != job_id
        || parsed.completed_at_unix_seconds != evidence.created_at_unix_seconds
        || customer.created_at_unix_seconds != evidence.created_at_unix_seconds
        || customer.created_at_unix_seconds < 0
    {
        return Err(PersistenceError::Storage(
            "generated-local customer metadata does not match its pack".into(),
        ));
    }
    Ok(())
}

fn insert_pack_registration(
    transaction: &rusqlite::Transaction<'_>,
    registration: &PackRegistration,
) -> Result<(), PersistenceError> {
    let pack = &registration.pack;
    transaction.execute(
        "INSERT INTO installed_packs(
            pack_id, title, version, manifest_sha256, archive_sha256,
            install_path, item_count, status, canonical_manifest, created_at_unix_seconds
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            pack.pack_id,
            pack.title,
            pack.version,
            pack.manifest_sha256,
            pack.archive_sha256,
            pack.install_path,
            pack.item_count,
            pack.status,
            pack.canonical_manifest,
            pack.created_at_unix_seconds,
        ],
    )?;
    if let Some(evidence) = &registration.generated_local_evidence {
        transaction.execute(
            "INSERT INTO generated_local_evidence(pack_id, generation_job_id, evidence_json, created_at_unix_seconds) VALUES(?1, ?2, ?3, ?4)",
            rusqlite::params![pack.pack_id, evidence.generation_job_id, evidence.evidence_json, evidence.created_at_unix_seconds],
        )?;
    }
    for item in &registration.items {
        transaction.execute(
            "INSERT INTO installed_items(item_id, pack_id, title) VALUES(?1, ?2, ?3)",
            rusqlite::params![item.item_id, pack.pack_id, item.title],
        )?;
    }
    for term in &registration.taxonomy {
        transaction.execute(
            "INSERT INTO installed_taxonomy(pack_id, kind, term_id, label) VALUES(?1, ?2, ?3, ?4)",
            rusqlite::params![pack.pack_id, term.kind, term.term_id, term.label],
        )?;
    }
    Ok(())
}

impl CatalogueRegistry for PreferencesRepository {
    fn list_installed_packs(&mut self) -> Result<Vec<InstalledPackRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT pack_id, title, version, manifest_sha256, archive_sha256,
                    install_path, item_count, status, canonical_manifest, created_at_unix_seconds
             FROM installed_packs ORDER BY title COLLATE NOCASE, pack_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(InstalledPackRecord {
                pack_id: row.get(0)?,
                title: row.get(1)?,
                version: row.get(2)?,
                manifest_sha256: row.get(3)?,
                archive_sha256: row.get(4)?,
                install_path: row.get(5)?,
                item_count: row.get(6)?,
                status: row.get(7)?,
                canonical_manifest: row.get(8)?,
                created_at_unix_seconds: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn find_installed_pack(
        &mut self,
        pack_id: &str,
    ) -> Result<Option<InstalledPackRecord>, PersistenceError> {
        Ok(self
            .connection
            .query_row(
                "SELECT pack_id, title, version, manifest_sha256, archive_sha256,
                        install_path, item_count, status, canonical_manifest, created_at_unix_seconds
                 FROM installed_packs WHERE pack_id = ?1",
                [pack_id],
                |row| {
                    Ok(InstalledPackRecord {
                        pack_id: row.get(0)?,
                        title: row.get(1)?,
                        version: row.get(2)?,
                        manifest_sha256: row.get(3)?,
                        archive_sha256: row.get(4)?,
                        install_path: row.get(5)?,
                        item_count: row.get(6)?,
                        status: row.get(7)?,
                        canonical_manifest: row.get(8)?,
                        created_at_unix_seconds: row.get(9)?,
                    })
                },
            )
            .optional()?)
    }

    fn find_existing_item_ids(
        &mut self,
        item_ids: &[String],
    ) -> Result<Vec<String>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT EXISTS(SELECT 1 FROM installed_items WHERE item_id = ?1)")?;
        let mut existing = Vec::new();
        for item_id in item_ids {
            let found: bool = statement.query_row([item_id], |row| row.get(0))?;
            if found {
                existing.push(item_id.clone());
            }
        }
        Ok(existing)
    }

    fn register_pack(&mut self, registration: &PackRegistration) -> Result<(), PersistenceError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_pack_registration(&transaction, registration)?;
        transaction.commit()?;
        Ok(())
    }

    fn replace_owner_waived_pack_preserving_feedback(
        &mut self,
        registration: &PackRegistration,
    ) -> Result<(), PersistenceError> {
        if registration.generated_local_evidence.is_some() {
            return Err(PersistenceError::Storage(
                "bundled pack upgrade cannot contain generated-local evidence".into(),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let status = transaction
            .query_row(
                "SELECT status FROM installed_packs WHERE pack_id=?1",
                [&registration.pack.pack_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if status.as_deref() != Some("owner_waived_bundled_private_beta") {
            return Err(PersistenceError::Storage(
                "only the matching owner-waived pack may be upgraded".into(),
            ));
        }
        let mut statement = transaction
            .prepare("SELECT item_id FROM installed_items WHERE pack_id=?1 ORDER BY item_id")?;
        let installed = statement
            .query_map([&registration.pack.pack_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut replacement = registration
            .items
            .iter()
            .map(|item| item.item_id.clone())
            .collect::<Vec<_>>();
        replacement.sort();
        if installed != replacement {
            return Err(PersistenceError::Storage(
                "bundled pack upgrade must preserve the exact item IDs".into(),
            ));
        }
        let pack = &registration.pack;
        let changed = transaction.execute(
            "UPDATE installed_packs SET title=?2,version=?3,manifest_sha256=?4,archive_sha256=?5,install_path=?6,item_count=?7,status=?8,canonical_manifest=?9,created_at_unix_seconds=?10 WHERE pack_id=?1 AND status='owner_waived_bundled_private_beta'",
            rusqlite::params![pack.pack_id, pack.title, pack.version, pack.manifest_sha256, pack.archive_sha256, pack.install_path, pack.item_count, pack.status, pack.canonical_manifest, pack.created_at_unix_seconds],
        )?;
        if changed != 1 {
            return Err(PersistenceError::Storage(
                "owner-waived pack changed during upgrade".into(),
            ));
        }
        for item in &registration.items {
            transaction.execute(
                "UPDATE installed_items SET title=?2 WHERE item_id=?1 AND pack_id=?3",
                rusqlite::params![item.item_id, item.title, pack.pack_id],
            )?;
        }
        transaction.execute(
            "DELETE FROM installed_taxonomy WHERE pack_id=?1",
            [&pack.pack_id],
        )?;
        for term in &registration.taxonomy {
            transaction.execute(
                "INSERT INTO installed_taxonomy(pack_id,kind,term_id,label) VALUES(?1,?2,?3,?4)",
                rusqlite::params![pack.pack_id, term.kind, term.term_id, term.label],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn register_generated_local_pack(
        &mut self,
        registration: &PackRegistration,
        customer: &GeneratedLocalCustomerRecord,
    ) -> Result<(), PersistenceError> {
        validate_generated_local_customer(registration, customer)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM installed_packs WHERE pack_id=?1)",
            [&registration.pack.pack_id],
            |row| row.get(0),
        )?;
        if exists {
            let stored_pack = transaction.query_row(
                "SELECT pack_id,title,version,manifest_sha256,archive_sha256,install_path,item_count,status,canonical_manifest,created_at_unix_seconds FROM installed_packs WHERE pack_id=?1",
                [&registration.pack.pack_id],
                |row| Ok(InstalledPackRecord { pack_id: row.get(0)?, title: row.get(1)?, version: row.get(2)?, manifest_sha256: row.get(3)?, archive_sha256: row.get(4)?, install_path: row.get(5)?, item_count: row.get(6)?, status: row.get(7)?, canonical_manifest: row.get(8)?, created_at_unix_seconds: row.get(9)? }),
            )?;
            let stored_evidence = transaction.query_row(
                "SELECT generation_job_id,evidence_json,created_at_unix_seconds FROM generated_local_evidence WHERE pack_id=?1",
                [&registration.pack.pack_id],
                |row| Ok(GeneratedLocalEvidenceRecord { generation_job_id: row.get(0)?, evidence_json: row.get(1)?, created_at_unix_seconds: row.get(2)? }),
            ).optional()?;
            let mut item_statement = transaction.prepare(
                "SELECT item_id,title FROM installed_items WHERE pack_id=?1 ORDER BY item_id",
            )?;
            let stored_items = item_statement
                .query_map([&registration.pack.pack_id], |row| {
                    Ok(RegisteredItem {
                        item_id: row.get(0)?,
                        title: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(item_statement);
            let mut taxonomy_statement = transaction.prepare(
                "SELECT kind,term_id,label FROM installed_taxonomy WHERE pack_id=?1 ORDER BY kind,term_id",
            )?;
            let stored_taxonomy = taxonomy_statement
                .query_map([&registration.pack.pack_id], |row| {
                    Ok(RegisteredTaxonomyTerm {
                        kind: row.get(0)?,
                        term_id: row.get(1)?,
                        label: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(taxonomy_statement);
            let stored_customer = transaction.query_row(
                "SELECT pack_id,item_id,title,activity,created_at_unix_seconds FROM generated_local_customer_metadata WHERE pack_id=?1",
                [&registration.pack.pack_id],
                |row| {
                    let activity: String = row.get(3)?;
                    Ok(GeneratedLocalCustomerRecord { pack_id: row.get(0)?, item_id: row.get(1)?, title: row.get(2)?, activity: Activity::from_storage_key(&activity).ok_or(rusqlite::Error::InvalidQuery)?, created_at_unix_seconds: row.get(4)? })
                },
            ).optional()?;
            let mut expected_taxonomy = registration.taxonomy.clone();
            expected_taxonomy.sort_by(|a, b| (&a.kind, &a.term_id).cmp(&(&b.kind, &b.term_id)));
            let mut expected_items = registration.items.clone();
            expected_items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
            if stored_pack != registration.pack
                || stored_evidence != registration.generated_local_evidence
                || stored_items != expected_items
                || stored_taxonomy != expected_taxonomy
                || stored_customer.as_ref() != Some(customer)
            {
                return Err(PersistenceError::Storage(
                    "conflicting generated-local registration".into(),
                ));
            }
            transaction.commit()?;
            return Ok(());
        }
        insert_pack_registration(&transaction, registration)?;
        transaction.execute(
            "INSERT INTO generated_local_customer_metadata(pack_id,item_id,title,activity,created_at_unix_seconds) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![customer.pack_id, customer.item_id, customer.title, customer.activity.storage_key(), customer.created_at_unix_seconds],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn find_generated_local_evidence(
        &mut self,
        pack_id: &str,
    ) -> Result<Option<GeneratedLocalEvidenceRecord>, PersistenceError> {
        self.connection
            .query_row(
                "SELECT generation_job_id, evidence_json, created_at_unix_seconds FROM generated_local_evidence WHERE pack_id = ?1",
                [pack_id],
                |row| Ok(GeneratedLocalEvidenceRecord { generation_job_id: row.get(0)?, evidence_json: row.get(1)?, created_at_unix_seconds: row.get(2)? }),
            )
            .optional()
            .map_err(Into::into)
    }

    fn list_generated_local_customers(
        &mut self,
    ) -> Result<Vec<GeneratedLocalCustomerRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT m.pack_id,m.item_id,m.title,m.activity,m.created_at_unix_seconds,
                    p.status,i.pack_id,e.generation_job_id,e.evidence_json,e.created_at_unix_seconds
             FROM generated_local_customer_metadata m
             LEFT JOIN installed_packs p ON p.pack_id=m.pack_id
             LEFT JOIN installed_items i ON i.item_id=m.item_id
             LEFT JOIN generated_local_evidence e ON e.pack_id=m.pack_id
             ORDER BY m.created_at_unix_seconds DESC,m.item_id DESC",
        )?;
        let rows = statement
            .query_map([], |row| {
                let activity: String = row.get(3)?;
                let record = GeneratedLocalCustomerRecord {
                    pack_id: row.get(0)?,
                    item_id: row.get(1)?,
                    title: row.get(2)?,
                    activity: Activity::from_storage_key(&activity)
                        .ok_or(rusqlite::Error::InvalidQuery)?,
                    created_at_unix_seconds: row.get(4)?,
                };
                let status: Option<String> = row.get(5)?;
                let item_pack_id: Option<String> = row.get(6)?;
                let evidence_job_id: Option<String> = row.get(7)?;
                let evidence_json: Option<String> = row.get(8)?;
                let evidence_created_at: Option<i64> = row.get(9)?;
                let job_id = record.pack_id.strip_prefix("generated.local.");
                let expected_item = format!("{}.item", record.pack_id);
                let parsed = evidence_json.as_deref().and_then(|value| {
                    serde_json::from_str::<StoredGeneratedLocalEvidence>(value).ok()
                });
                if validate_customer_title(&record.title).is_err()
                    || record.created_at_unix_seconds < 0
                    || !valid_identifier(&record.pack_id)
                    || !valid_identifier(&record.item_id)
                    || job_id.is_none_or(|value| !valid_identifier(value))
                    || record.item_id != expected_item
                    || status.as_deref() != Some("generated_local")
                    || item_pack_id.as_deref() != Some(record.pack_id.as_str())
                    || evidence_job_id.as_deref() != job_id
                    || evidence_created_at != Some(record.created_at_unix_seconds)
                    || parsed.as_ref().is_none_or(|value| {
                        value.producer != GENERATED_LOCAL_EVIDENCE_PRODUCER
                            || Some(value.job_id.as_str()) != job_id
                            || value.completed_at_unix_seconds != record.created_at_unix_seconds
                    })
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(record)
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into);
        rows
    }

    fn rename_generated_local_customer(
        &mut self,
        item_id: &str,
        title: &str,
    ) -> Result<(), PersistenceError> {
        validate_customer_title(title)?;
        let changed = self.connection.execute(
            "UPDATE generated_local_customer_metadata SET title = ?2 WHERE item_id = ?1",
            rusqlite::params![item_id, title],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(PersistenceError::UnknownInstalledItem(item_id.into()))
        }
    }

    fn unregister_generated_local(
        &mut self,
        item_id: &str,
    ) -> Result<Option<InstalledPackRecord>, PersistenceError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = transaction.query_row(
            "SELECT p.pack_id,p.title,p.version,p.manifest_sha256,p.archive_sha256,p.install_path,p.item_count,p.status,p.canonical_manifest,p.created_at_unix_seconds FROM installed_packs p JOIN generated_local_customer_metadata m ON m.pack_id=p.pack_id WHERE m.item_id=?1 AND p.status='generated_local'",
            [item_id], |row| Ok(InstalledPackRecord { pack_id: row.get(0)?, title: row.get(1)?, version: row.get(2)?, manifest_sha256: row.get(3)?, archive_sha256: row.get(4)?, install_path: row.get(5)?, item_count: row.get(6)?, status: row.get(7)?, canonical_manifest: row.get(8)?, created_at_unix_seconds: row.get(9)? }),
        ).optional()?;
        if let Some(ref record) = record {
            transaction.execute(
                "DELETE FROM installed_packs WHERE pack_id=?1",
                [&record.pack_id],
            )?;
        }
        transaction.commit()?;
        Ok(record)
    }
}

fn validate_customer_title(title: &str) -> Result<(), PersistenceError> {
    if title != title.trim()
        || !(1..=100).contains(&title.chars().count())
        || title.chars().any(|c| c.is_control())
    {
        return Err(PersistenceError::Storage(
            "generated music title must be 1–100 characters with no control characters".into(),
        ));
    }
    Ok(())
}

impl ItemFeedbackStore for PreferencesRepository {
    fn load_item_feedback(
        &mut self,
        activity: Activity,
        item_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, TrackFeedback>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT item_id, activity, feedback FROM item_activity_feedback WHERE item_id = ?1",
        )?;
        let mut feedback = std::collections::BTreeMap::new();
        for item_id in item_ids {
            if !valid_identifier(item_id) {
                return Err(PersistenceError::InvalidItemId(item_id.clone()));
            }
            let rows = statement.query_map([item_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (stored_id, stored_activity, stored_feedback) = row?;
                if !valid_identifier(&stored_id) {
                    return Err(PersistenceError::InvalidItemId(stored_id));
                }
                let stored_activity = Activity::from_storage_key(&stored_activity)
                    .ok_or(PersistenceError::InvalidActivity(stored_activity))?;
                let parsed = TrackFeedback::from_storage_key(&stored_feedback)
                    .ok_or(PersistenceError::InvalidTrackFeedback(stored_feedback))?;
                if stored_activity == activity {
                    feedback.insert(stored_id, parsed);
                }
            }
        }
        Ok(feedback)
    }

    fn save_item_feedback(
        &mut self,
        item_id: &str,
        activity: Activity,
        feedback: TrackFeedback,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(item_id) {
            return Err(PersistenceError::InvalidItemId(item_id.to_owned()));
        }
        let changed = self.connection.execute(
            "INSERT INTO item_activity_feedback(item_id, activity, feedback, updated_at_unix_seconds)
             SELECT item_id, ?2, ?3, unixepoch() FROM installed_items WHERE item_id = ?1
             ON CONFLICT(item_id, activity) DO UPDATE SET
                 feedback = excluded.feedback,
                 updated_at_unix_seconds = excluded.updated_at_unix_seconds",
            rusqlite::params![item_id, activity.storage_key(), feedback.storage_key()],
        )?;
        if changed == 0 {
            return Err(PersistenceError::UnknownInstalledItem(item_id.to_owned()));
        }
        Ok(())
    }

    fn clear_item_feedback(
        &mut self,
        item_id: &str,
        activity: Activity,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(item_id) {
            return Err(PersistenceError::InvalidItemId(item_id.to_owned()));
        }
        self.connection.execute(
            "DELETE FROM item_activity_feedback WHERE item_id = ?1 AND activity = ?2",
            rusqlite::params![item_id, activity.storage_key()],
        )?;
        Ok(())
    }

    fn load_item_enjoyment(
        &mut self,
        activity: Activity,
        item_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, TrackEnjoyment>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT item_id, activity, enjoyment FROM item_activity_enjoyment WHERE item_id = ?1",
        )?;
        let mut enjoyment = std::collections::BTreeMap::new();
        for item_id in item_ids {
            if !valid_identifier(item_id) {
                return Err(PersistenceError::InvalidItemId(item_id.clone()));
            }
            for row in statement.query_map([item_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })? {
                let (stored_id, stored_activity, stored_enjoyment) = row?;
                if !valid_identifier(&stored_id) {
                    return Err(PersistenceError::InvalidItemId(stored_id));
                }
                let stored_activity = Activity::from_storage_key(&stored_activity)
                    .ok_or(PersistenceError::InvalidActivity(stored_activity))?;
                let parsed = TrackEnjoyment::from_storage_key(&stored_enjoyment)
                    .ok_or(PersistenceError::InvalidTrackEnjoyment(stored_enjoyment))?;
                if stored_activity == activity {
                    enjoyment.insert(stored_id, parsed);
                }
            }
        }
        Ok(enjoyment)
    }

    fn save_item_enjoyment(
        &mut self,
        item_id: &str,
        activity: Activity,
        enjoyment: TrackEnjoyment,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(item_id) {
            return Err(PersistenceError::InvalidItemId(item_id.to_owned()));
        }
        let changed = self.connection.execute("INSERT INTO item_activity_enjoyment(item_id, activity, enjoyment, updated_at_unix_seconds) SELECT item_id, ?2, ?3, unixepoch() FROM installed_items WHERE item_id = ?1 ON CONFLICT(item_id, activity) DO UPDATE SET enjoyment = excluded.enjoyment, updated_at_unix_seconds = excluded.updated_at_unix_seconds", rusqlite::params![item_id, activity.storage_key(), enjoyment.storage_key()])?;
        if changed == 0 {
            return Err(PersistenceError::UnknownInstalledItem(item_id.to_owned()));
        }
        Ok(())
    }

    fn clear_item_enjoyment(
        &mut self,
        item_id: &str,
        activity: Activity,
    ) -> Result<(), PersistenceError> {
        if !valid_identifier(item_id) {
            return Err(PersistenceError::InvalidItemId(item_id.to_owned()));
        }
        self.connection.execute(
            "DELETE FROM item_activity_enjoyment WHERE item_id = ?1 AND activity = ?2",
            rusqlite::params![item_id, activity.storage_key()],
        )?;
        Ok(())
    }
}

fn apply_migrations(connection: &mut Connection) -> Result<(), PersistenceError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );",
    )?;

    for &(version, name, sql) in MIGRATIONS {
        let already_applied: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [version],
            |row| row.get(0),
        )?;
        if already_applied {
            continue;
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name) VALUES(?1, ?2)",
            rusqlite::params![version, name],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use music_studio_domain::{
        build_studio_prompt, StudioDuration, StudioEnergy, StudioId, StudioPromptInput,
    };

    fn studio_job(number: char, timestamp: u64) -> StudioJobRecord {
        let request = StudioPromptInput::new(
            Activity::DeepWork,
            StudioId::new("ambient").unwrap(),
            None,
            StudioEnergy::Medium,
            vec![],
            None,
            None,
            StudioDuration::Seconds180,
        )
        .unwrap();
        let prompt = build_studio_prompt(&request, 42).unwrap();
        StudioJobRecord::new(
            StudioJobId::new(format!("job_{number}bcdefghijkl")).unwrap(),
            music_studio_domain::StudioAttemptId::new(format!("attempt_{number}bcdefghijkl"))
                .unwrap(),
            request,
            prompt,
            timestamp,
        )
        .unwrap()
    }

    fn ready_artifact(job: &StudioJobRecord) -> StudioJobArtifact {
        StudioJobArtifact {
            job_id: job.job_id.clone(),
            parent_job_id: None,
            runtime_version: "runtime-v1".into(),
            stage: "ready".into(),
            output_relative_path: Some(format!("drafts/{}.flac", job.job_id.as_str())),
            output_sha256: Some("a".repeat(64)),
            analysis_json: Some(r#"{"schema_version":1,"vocal_speech":"not_assessed"}"#.into()),
            safe_error_code: None,
            created_at_ms: job.created_at_ms,
            updated_at_ms: job.updated_at_ms + 1,
        }
    }

    fn generated_registration(job_id: &str) -> (PackRegistration, GeneratedLocalCustomerRecord) {
        let pack_id = format!("generated.local.{job_id}");
        let item_id = format!("{pack_id}.item");
        let created_at = 42;
        (
            PackRegistration {
                pack: InstalledPackRecord {
                    pack_id: pack_id.clone(),
                    title: "Deep current".into(),
                    version: "1.0.0".into(),
                    manifest_sha256: "a".repeat(64),
                    archive_sha256: "b".repeat(64),
                    install_path: format!("packs/{pack_id}/version"),
                    item_count: 1,
                    status: "generated_local".into(),
                    canonical_manifest: "{}".into(),
                    created_at_unix_seconds: 0,
                },
                items: vec![RegisteredItem {
                    item_id: item_id.clone(),
                    title: "Deep current".into(),
                }],
                taxonomy: vec![],
                generated_local_evidence: Some(GeneratedLocalEvidenceRecord {
                    generation_job_id: job_id.into(),
                    evidence_json: format!(
                        r#"{{"producer":"adhd-music-studio","job_id":"{job_id}","completed_at_unix_seconds":{created_at}}}"#
                    ),
                    created_at_unix_seconds: created_at,
                }),
            },
            GeneratedLocalCustomerRecord {
                pack_id,
                item_id,
                title: "Deep current".into(),
                activity: Activity::DeepWork,
                created_at_unix_seconds: created_at,
            },
        )
    }

    #[test]
    fn generated_local_registration_is_atomic_exact_and_strict_on_read() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let (registration, customer) = generated_registration("job_abcdefghijkl");
        repository
            .register_generated_local_pack(&registration, &customer)
            .unwrap();
        repository
            .register_generated_local_pack(&registration, &customer)
            .unwrap();
        assert_eq!(
            repository.list_generated_local_customers().unwrap(),
            vec![customer.clone()]
        );

        let mut conflict = customer.clone();
        conflict.title = "Different".into();
        assert!(repository
            .register_generated_local_pack(&registration, &conflict)
            .is_err());
        assert_eq!(
            repository.list_generated_local_customers().unwrap(),
            vec![customer.clone()]
        );

        repository
            .connection
            .execute_batch(
                "DROP TRIGGER generated_local_customer_metadata_update_guard;
                 UPDATE generated_local_customer_metadata SET created_at_unix_seconds=-1;",
            )
            .unwrap();
        assert!(repository.list_generated_local_customers().is_err());
    }

    #[test]
    fn generated_local_registration_rolls_back_pack_when_customer_is_invalid() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let (registration, mut customer) = generated_registration("job_bcdefghijklm");
        customer.item_id = "generated.local.other.item".into();
        assert!(repository
            .register_generated_local_pack(&registration, &customer)
            .is_err());
        assert!(repository
            .find_installed_pack(&registration.pack.pack_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn generated_local_registration_rejects_the_obsolete_evidence_producer() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let (mut registration, customer) = generated_registration("job_cdefghijklmn");
        registration
            .generated_local_evidence
            .as_mut()
            .unwrap()
            .evidence_json = r#"{"producer":"music_studio","job_id":"job_cdefghijklmn","completed_at_unix_seconds":42}"#.into();

        assert!(repository
            .register_generated_local_pack(&registration, &customer)
            .is_err());
        assert!(repository
            .find_installed_pack(&registration.pack.pack_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn removing_a_studio_job_detaches_children_and_cascades_its_artifact() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let parent = studio_job('p', 10);
        let child = studio_job('c', 11);
        repository.create_studio_job(&parent).unwrap();
        repository.create_studio_job(&child).unwrap();
        repository
            .upsert_studio_job_artifact(&ready_artifact(&parent))
            .unwrap();
        let mut child_artifact = ready_artifact(&child);
        child_artifact.parent_job_id = Some(parent.job_id.clone());
        repository
            .upsert_studio_job_artifact(&child_artifact)
            .unwrap();

        repository.remove_studio_job(&parent.job_id).unwrap();
        assert!(repository
            .load_studio_job(&parent.job_id)
            .unwrap()
            .is_none());
        assert!(repository
            .load_studio_job_artifact(&parent.job_id)
            .unwrap()
            .is_none());
        assert_eq!(
            repository
                .load_studio_job_artifact(&child.job_id)
                .unwrap()
                .unwrap()
                .parent_job_id,
            None
        );
    }

    #[test]
    fn migration_is_versioned_and_idempotent() {
        let repository = PreferencesRepository::in_memory().unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
    }

    #[test]
    fn onboarding_is_empty_on_fresh_storage_and_is_strict_durable_and_ordered() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert!(!repository.onboarding_preferences().unwrap().completed);
        repository
            .complete_onboarding(Intensity::High, &["nature".into(), "drone".into()])
            .unwrap();
        assert_eq!(
            repository.onboarding_preferences().unwrap(),
            OnboardingPreferences {
                completed: true,
                intensity: Intensity::High,
                genres: vec!["drone".into(), "nature".into()]
            }
        );
        assert!(matches!(
            repository.complete_onboarding(
                Intensity::Low,
                &[
                    "drone".into(),
                    "nature".into(),
                    "piano".into(),
                    "acoustic".into()
                ]
            ),
            Err(PersistenceError::TooManyOnboardingGenres)
        ));
        assert!(matches!(
            repository.complete_onboarding(Intensity::Low, &["not-a-product-genre".into()]),
            Err(PersistenceError::InvalidGenreId(_))
        ));
        assert!(matches!(
            repository.complete_onboarding(Intensity::Off, &[]),
            Err(PersistenceError::InvalidIntensity(value)) if value == "off"
        ));
        repository
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE onboarding_preferences SET intensity = 'off';")
            .unwrap();
        assert!(matches!(
            repository.onboarding_preferences(),
            Err(PersistenceError::InvalidIntensity(value)) if value == "off"
        ));
    }

    #[test]
    fn global_master_volume_round_trips_and_corruption_is_visible() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert_eq!(repository.load_master_volume().unwrap(), None);
        repository
            .save_master_volume(MasterVolume::new(42).unwrap())
            .unwrap();
        assert_eq!(
            repository.load_master_volume().unwrap().unwrap().percent(),
            42
        );
        repository
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE application_preferences SET master_volume = 101;")
            .unwrap();
        assert!(matches!(
            repository.load_master_volume(),
            Err(PersistenceError::InvalidMasterVolume(101))
        ));
    }

    #[test]
    fn genre_preferences_are_isolated_and_survive_reopen() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut repository = PreferencesRepository::open(file.path()).unwrap();
            repository
                .save_genre_preference(Activity::DeepWork, "ambient")
                .unwrap();
            repository
                .save_genre_preference(Activity::Learning, "classical")
                .unwrap();
            repository
                .clear_genre_preference(Activity::Learning)
                .unwrap();
        }
        let mut reopened = PreferencesRepository::open(file.path()).unwrap();
        assert_eq!(
            reopened
                .load_genre_preference(Activity::DeepWork)
                .unwrap()
                .as_deref(),
            Some("ambient")
        );
        assert_eq!(
            reopened.load_genre_preference(Activity::Learning).unwrap(),
            None
        );
    }

    #[test]
    fn malformed_genre_ids_are_rejected_on_write_and_read() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert!(matches!(
            repository.save_genre_preference(Activity::DeepWork, "Bad genre"),
            Err(PersistenceError::InvalidGenreId(_))
        ));
        repository.connection.execute("INSERT INTO activity_genre_preferences(activity, genre_id) VALUES('deep_work', 'Bad genre')", []).unwrap();
        assert!(
            matches!(repository.load_genre_preference(Activity::DeepWork), Err(PersistenceError::InvalidGenreId(value)) if value == "Bad genre")
        );
    }

    #[test]
    fn mood_preferences_are_isolated_and_corrupt_rows_are_reported() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut repository = PreferencesRepository::open(file.path()).unwrap();
            repository
                .save_mood_preference(Activity::DeepWork, "steady")
                .unwrap();
            repository
                .save_mood_preference(Activity::Learning, "calm")
                .unwrap();
        }
        let mut reopened = PreferencesRepository::open(file.path()).unwrap();
        assert_eq!(
            reopened
                .load_mood_preference(Activity::DeepWork)
                .unwrap()
                .as_deref(),
            Some("steady")
        );
        reopened.connection.execute("UPDATE activity_mood_preferences SET mood_id = 'Bad mood' WHERE activity = 'learning'", []).unwrap();
        assert!(
            matches!(reopened.load_mood_preference(Activity::Learning), Err(PersistenceError::InvalidMoodId(value)) if value == "Bad mood")
        );
    }

    fn insert_feedback_item(repository: &mut PreferencesRepository, item_id: &str) {
        repository.connection.execute("INSERT INTO installed_packs(pack_id, title, version, manifest_sha256, archive_sha256, install_path, item_count, status, canonical_manifest) VALUES('test-pack', 'Test', '1.0.0', 'a', 'b', 'path', 1, 'validated_metadata', '{}')", []).unwrap();
        repository.connection.execute(
            "INSERT INTO installed_items(item_id, pack_id, title) VALUES(?1, 'test-pack', 'Track')",
            [item_id],
        ).unwrap();
    }

    #[test]
    fn item_feedback_is_activity_scoped_overwritable_and_survives_reopen() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut repository = PreferencesRepository::open(file.path()).unwrap();
            insert_feedback_item(&mut repository, "track-one");
            repository
                .save_item_feedback("track-one", Activity::DeepWork, TrackFeedback::HelpsFocus)
                .unwrap();
            repository
                .save_item_feedback("track-one", Activity::LightWork, TrackFeedback::Neutral)
                .unwrap();
            repository
                .save_item_feedback("track-one", Activity::DeepWork, TrackFeedback::Distracting)
                .unwrap();
        }
        let mut reopened = PreferencesRepository::open(file.path()).unwrap();
        let ids = ["track-one".to_owned()];
        assert_eq!(
            reopened
                .load_item_feedback(Activity::DeepWork, &ids)
                .unwrap()
                .get("track-one"),
            Some(&TrackFeedback::Distracting)
        );
        assert_eq!(
            reopened
                .load_item_feedback(Activity::LightWork, &ids)
                .unwrap()
                .get("track-one"),
            Some(&TrackFeedback::Neutral)
        );
    }

    #[test]
    fn item_feedback_rejects_unknown_and_corrupt_rows_and_cascades_on_pack_removal() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert!(matches!(
            repository.save_item_feedback("missing", Activity::DeepWork, TrackFeedback::Neutral),
            Err(PersistenceError::UnknownInstalledItem(_))
        ));
        insert_feedback_item(&mut repository, "track-one");
        repository
            .save_item_feedback("track-one", Activity::DeepWork, TrackFeedback::Neutral)
            .unwrap();
        repository
            .connection
            .execute(
                "DELETE FROM installed_packs WHERE pack_id = 'test-pack'",
                [],
            )
            .unwrap();
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM item_activity_feedback", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );

        insert_feedback_item(&mut repository, "track-two");
        repository
            .connection
            .execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        repository.connection.execute("INSERT INTO item_activity_feedback(item_id, activity, feedback, updated_at_unix_seconds) VALUES('track-two', 'deep_work', 'wrong', 0)", []).unwrap();
        assert!(
            matches!(repository.load_item_feedback(Activity::DeepWork, &["track-two".to_owned()]), Err(PersistenceError::InvalidTrackFeedback(value)) if value == "wrong")
        );
        repository
            .connection
            .execute("DELETE FROM item_activity_feedback", [])
            .unwrap();
        repository
            .connection
            .execute(
                "INSERT INTO item_activity_feedback(item_id, activity, feedback, updated_at_unix_seconds) VALUES('track-two', 'not_an_activity', 'neutral', 0)",
                [],
            )
            .unwrap();
        assert!(matches!(
            repository.load_item_feedback(Activity::DeepWork, &["track-two".to_owned()]),
            Err(PersistenceError::InvalidActivity(value)) if value == "not_an_activity"
        ));
    }

    #[test]
    fn enjoyment_is_independent_activity_scoped_corruption_is_visible_and_cascades() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        insert_feedback_item(&mut repository, "track-enjoyment");
        repository
            .save_item_feedback(
                "track-enjoyment",
                Activity::DeepWork,
                TrackFeedback::HelpsFocus,
            )
            .unwrap();
        repository
            .save_item_enjoyment(
                "track-enjoyment",
                Activity::DeepWork,
                TrackEnjoyment::NotForMe,
            )
            .unwrap();
        repository
            .save_item_enjoyment("track-enjoyment", Activity::Learning, TrackEnjoyment::Liked)
            .unwrap();
        let ids = ["track-enjoyment".to_owned()];
        assert_eq!(
            repository
                .load_item_feedback(Activity::DeepWork, &ids)
                .unwrap()
                .get("track-enjoyment"),
            Some(&TrackFeedback::HelpsFocus)
        );
        assert_eq!(
            repository
                .load_item_enjoyment(Activity::DeepWork, &ids)
                .unwrap()
                .get("track-enjoyment"),
            Some(&TrackEnjoyment::NotForMe)
        );
        repository
            .connection
            .execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        repository.connection.execute("UPDATE item_activity_enjoyment SET enjoyment = 'corrupt' WHERE activity = 'learning'", []).unwrap();
        assert!(
            matches!(repository.load_item_enjoyment(Activity::Learning, &ids), Err(PersistenceError::InvalidTrackEnjoyment(value)) if value == "corrupt")
        );
        repository
            .connection
            .execute(
                "DELETE FROM installed_packs WHERE pack_id = 'test-pack'",
                [],
            )
            .unwrap();
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM item_activity_enjoyment", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn last_activity_and_per_activity_intensities_round_trip() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert_eq!(repository.load_last_activity().unwrap(), None);
        assert_eq!(repository.load_intensity(Activity::DeepWork).unwrap(), None);

        repository.save_last_activity(Activity::Creativity).unwrap();
        repository
            .save_intensity(Activity::DeepWork, Intensity::Low)
            .unwrap();
        repository
            .save_intensity(Activity::Creativity, Intensity::High)
            .unwrap();

        assert_eq!(
            repository.load_last_activity().unwrap(),
            Some(Activity::Creativity)
        );
        assert_eq!(
            repository.load_intensity(Activity::DeepWork).unwrap(),
            Some(Intensity::Low)
        );
        assert_eq!(
            repository.load_intensity(Activity::Creativity).unwrap(),
            Some(Intensity::High)
        );
        assert_eq!(repository.load_intensity(Activity::Learning).unwrap(), None);
    }

    #[test]
    fn per_activity_timer_configs_round_trip_with_explicit_columns() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        assert_eq!(
            repository.load_session_type(Activity::DeepWork).unwrap(),
            None
        );
        repository
            .save_session_type(
                Activity::DeepWork,
                SessionType::Countdown { seconds: 1_500 },
            )
            .unwrap();
        repository
            .save_session_type(
                Activity::Learning,
                SessionType::Interval {
                    work_seconds: 1_500,
                    break_seconds: 300,
                    repeats: 4,
                },
            )
            .unwrap();
        assert_eq!(
            repository.load_session_type(Activity::DeepWork).unwrap(),
            Some(SessionType::Countdown { seconds: 1_500 })
        );
        assert_eq!(
            repository.load_session_type(Activity::Learning).unwrap(),
            Some(SessionType::Interval {
                work_seconds: 1_500,
                break_seconds: 300,
                repeats: 4,
            })
        );
        assert!(repository
            .save_session_type(Activity::Creativity, SessionType::Countdown { seconds: 0 })
            .is_err());
    }

    #[test]
    fn invalid_stored_value_is_reported_instead_of_defaulted() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        repository
            .connection
            .execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        repository
            .connection
            .execute(
                "INSERT INTO application_preferences(singleton, last_activity) VALUES(1, 'invalid')",
                [],
            )
            .unwrap();

        assert!(matches!(
            repository.load_last_activity(),
            Err(PersistenceError::InvalidActivity(value)) if value == "invalid"
        ));
    }

    #[test]
    fn migration_upgrades_a_previous_version_database_without_losing_preferences() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE
                );",
            )
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0001_activity_preferences.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name) VALUES(1, 'activity_preferences')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO application_preferences(singleton, last_activity)
                 VALUES(1, 'learning')",
                [],
            )
            .unwrap();

        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
        assert_eq!(
            repository.load_last_activity().unwrap(),
            Some(Activity::Learning)
        );
        assert!(repository.list_installed_packs().unwrap().is_empty());
        assert_eq!(
            repository.load_session_type(Activity::Learning).unwrap(),
            None
        );
        assert!(repository.onboarding_preferences().unwrap().completed);
    }

    #[test]
    fn migration_upgrades_v11_database_and_makes_studio_jobs_usable() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE
            );",
            )
            .unwrap();
        for (version, name, sql) in MIGRATIONS.iter().take(11) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name) VALUES(?1, ?2)",
                    rusqlite::params![version, name],
                )
                .unwrap();
        }
        connection.execute(
            "INSERT INTO application_preferences(singleton, last_activity) VALUES(1, 'learning')",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO session_history(id, activity, intensity, session_type, started_at) VALUES('0123456789abcdef0123456789abcdef', 'learning', 'medium', '{}', 5)",
            [],
        ).unwrap();

        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
        assert_eq!(
            repository.load_last_activity().unwrap(),
            Some(Activity::Learning)
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM session_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let job = studio_job('z', 10);
        repository.create_studio_job(&job).unwrap();
        assert_eq!(repository.load_studio_job(&job.job_id).unwrap(), Some(job));
    }

    #[test]
    fn migration_upgrades_v12_and_preserves_existing_registry_records() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE
                );",
            )
            .unwrap();
        for (version, name, sql) in MIGRATIONS.iter().take(12) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name) VALUES(?1, ?2)",
                    rusqlite::params![version, name],
                )
                .unwrap();
        }
        connection.execute(
            "INSERT INTO installed_packs(pack_id, title, version, manifest_sha256, archive_sha256, install_path, item_count, status, canonical_manifest) VALUES('preserved.v12.pack', 'Preserved v12', '1.0.0', ?1, ?2, 'packs/preserved', 1, 'validated_metadata', '{}')",
            rusqlite::params!["a".repeat(64), "b".repeat(64)],
        ).unwrap();
        connection.execute(
            "INSERT INTO installed_items(item_id, pack_id, title) VALUES('preserved-v12-item', 'preserved.v12.pack', 'Preserved item')",
            [],
        ).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        connection.execute(
            "INSERT INTO installed_items(item_id, pack_id, title) VALUES('orphaned-v12-item', 'missing.v12.pack', 'Unreachable item')",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO installed_taxonomy(pack_id, kind, term_id, label) VALUES('missing.v12.pack', 'genre', 'orphaned-genre', 'Unreachable genre')",
            [],
        ).unwrap();

        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
        assert_eq!(
            repository
                .find_installed_pack("preserved.v12.pack")
                .unwrap(),
            Some(InstalledPackRecord {
                pack_id: "preserved.v12.pack".to_owned(),
                title: "Preserved v12".to_owned(),
                version: "1.0.0".to_owned(),
                manifest_sha256: "a".repeat(64),
                archive_sha256: "b".repeat(64),
                install_path: "packs/preserved".to_owned(),
                item_count: 1,
                status: "validated_metadata".to_owned(),
                canonical_manifest: "{}".to_owned(),
                created_at_unix_seconds: 0,
            })
        );
        assert_eq!(
            repository
                .find_existing_item_ids(&[
                    "preserved-v12-item".to_owned(),
                    "orphaned-v12-item".to_owned(),
                ])
                .unwrap(),
            vec!["preserved-v12-item".to_owned()]
        );
        assert!(repository
            .connection
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap()
            .is_none());
    }

    #[test]
    fn migration_upgrades_v2_without_losing_catalogue_records() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE
                );",
            )
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0001_activity_preferences.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0002_installed_catalogue.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name) VALUES
                 (1, 'activity_preferences'), (2, 'installed_catalogue')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO installed_packs(
                    pack_id, title, version, manifest_sha256, archive_sha256,
                    install_path, item_count, status, canonical_manifest
                 ) VALUES('preserved.pack', 'Preserved', '1.0.0', ?1, ?2,
                          'internal/preserved', 0, 'validated_metadata', '{}')",
                rusqlite::params!["a".repeat(64), "b".repeat(64)],
            )
            .unwrap();

        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
        assert!(repository
            .find_installed_pack("preserved.pack")
            .unwrap()
            .is_some());
        assert_eq!(
            repository.load_session_type(Activity::DeepWork).unwrap(),
            None
        );
    }

    #[test]
    fn migration_upgrades_v7_without_losing_any_focus_feedback() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);").unwrap();
        for (version, name, sql) in MIGRATIONS.iter().take(7) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name) VALUES(?1, ?2)",
                    rusqlite::params![version, name],
                )
                .unwrap();
        }
        connection.execute("INSERT INTO installed_packs(pack_id, title, version, manifest_sha256, archive_sha256, install_path, item_count, status, canonical_manifest) VALUES('legacy-pack', 'Legacy', '1', 'a', 'b', 'path', 2, 'validated_metadata', '{}')", []).unwrap();
        connection.execute("INSERT INTO installed_items(item_id, pack_id, title) VALUES('helpful-track', 'legacy-pack', 'Helpful'), ('neutral-track', 'legacy-pack', 'Neutral')", []).unwrap();
        connection.execute("INSERT INTO item_activity_feedback(item_id, activity, feedback, updated_at_unix_seconds) VALUES('helpful-track', 'deep_work', 'helps_focus', 0), ('neutral-track', 'deep_work', 'neutral', 0)", []).unwrap();
        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        let ids = ["helpful-track".to_owned(), "neutral-track".to_owned()];
        assert_eq!(repository.schema_version().unwrap(), 16);
        assert_eq!(
            repository
                .load_item_feedback(Activity::DeepWork, &ids)
                .unwrap()
                .get("helpful-track"),
            Some(&TrackFeedback::HelpsFocus)
        );
        assert_eq!(
            repository
                .load_item_feedback(Activity::DeepWork, &ids)
                .unwrap()
                .get("neutral-track"),
            Some(&TrackFeedback::Neutral)
        );
        assert!(repository
            .load_item_enjoyment(Activity::DeepWork, &ids)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn installed_pack_registry_is_transactional_and_queryable() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let registration = PackRegistration {
            pack: InstalledPackRecord {
                pack_id: "test.pack".to_owned(),
                title: "Test Pack".to_owned(),
                version: "1.0.0".to_owned(),
                manifest_sha256: "a".repeat(64),
                archive_sha256: "b".repeat(64),
                install_path: "internal/test.pack/1.0.0".to_owned(),
                item_count: 1,
                status: "validated_metadata".to_owned(),
                canonical_manifest: "{}".to_owned(),
                created_at_unix_seconds: 0,
            },
            items: vec![RegisteredItem {
                item_id: "test-item".to_owned(),
                title: "Test item".to_owned(),
            }],
            taxonomy: vec![RegisteredTaxonomyTerm {
                kind: "genre".to_owned(),
                term_id: "test-genre".to_owned(),
                label: "Test Genre".to_owned(),
            }],
            generated_local_evidence: None,
        };
        repository.register_pack(&registration).unwrap();
        assert_eq!(repository.list_installed_packs().unwrap().len(), 1);
        assert!(repository
            .find_installed_pack("test.pack")
            .unwrap()
            .is_some());
        assert_eq!(
            repository
                .find_existing_item_ids(&["test-item".to_owned(), "other".to_owned()])
                .unwrap(),
            vec!["test-item".to_owned()]
        );

        assert!(repository.register_pack(&registration).is_err());
        assert_eq!(repository.list_installed_packs().unwrap().len(), 1);
    }

    #[test]
    fn bundled_upgrade_preserves_feedback_and_requires_exact_track_identity() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let mut registration = PackRegistration {
            pack: InstalledPackRecord {
                pack_id: "aria.library".to_owned(),
                title: "Listening test".to_owned(),
                version: "0.1.0".to_owned(),
                manifest_sha256: "a".repeat(64),
                archive_sha256: "b".repeat(64),
                install_path: "packs/aria.library/0.1.0".to_owned(),
                item_count: 1,
                status: "owner_waived_bundled_private_beta".to_owned(),
                canonical_manifest: "{}".to_owned(),
                created_at_unix_seconds: 1,
            },
            items: vec![RegisteredItem {
                item_id: "same-track".to_owned(),
                title: "Old title".to_owned(),
            }],
            taxonomy: vec![],
            generated_local_evidence: None,
        };
        repository.register_pack(&registration).unwrap();
        repository
            .save_item_feedback("same-track", Activity::DeepWork, TrackFeedback::HelpsFocus)
            .unwrap();
        repository
            .save_item_enjoyment("same-track", Activity::DeepWork, TrackEnjoyment::Liked)
            .unwrap();

        let mut invalid = registration.clone();
        invalid.pack.status = "validated_metadata".to_owned();
        invalid.items[0].item_id = "changed-track".to_owned();
        assert!(repository
            .replace_owner_waived_pack_preserving_feedback(&invalid)
            .is_err());
        assert_eq!(
            repository
                .find_installed_pack("aria.library")
                .unwrap()
                .unwrap()
                .status,
            "owner_waived_bundled_private_beta"
        );

        registration.pack.title = "Aria Focus Library".to_owned();
        registration.pack.version = "1.0.0".to_owned();
        registration.pack.status = "validated_metadata".to_owned();
        registration.pack.install_path = "packs/aria.library/1.0.0".to_owned();
        registration.items[0].title = "Reviewed title".to_owned();
        repository
            .replace_owner_waived_pack_preserving_feedback(&registration)
            .unwrap();

        assert_eq!(
            repository
                .find_installed_pack("aria.library")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
        let ids = ["same-track".to_owned()];
        assert_eq!(
            repository
                .load_item_feedback(Activity::DeepWork, &ids)
                .unwrap()
                .get("same-track"),
            Some(&TrackFeedback::HelpsFocus)
        );
        assert_eq!(
            repository
                .load_item_enjoyment(Activity::DeepWork, &ids)
                .unwrap()
                .get("same-track"),
            Some(&TrackEnjoyment::Liked)
        );
    }

    #[test]
    fn brand_path_rebase_handles_owner_waived_v1_v2_and_preserves_user_data() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("com.adhdmusic.desktop");
        let current = temp.path().join("com.ariazanganeh.ariafocus");
        std::fs::create_dir_all(&current).unwrap();
        let mut repository = PreferencesRepository::in_memory().unwrap();
        for (suffix, version) in [('1', "1.0.0"), ('2', "2.0.0")] {
            let pack_id = format!("aria.library.v{suffix}");
            let item_id = format!("aria-track-v{suffix}");
            let relative = PathBuf::from("content")
                .join("packs")
                .join(&pack_id)
                .join(suffix.to_string().repeat(64));
            std::fs::create_dir_all(current.join(&relative)).unwrap();
            repository
                .register_pack(&PackRegistration {
                    pack: InstalledPackRecord {
                        pack_id,
                        title: format!("Library v{suffix}"),
                        version: version.into(),
                        manifest_sha256: "a".repeat(64),
                        archive_sha256: "b".repeat(64),
                        install_path: legacy.join(relative).to_string_lossy().into_owned(),
                        item_count: 1,
                        status: "owner_waived_bundled_private_beta".into(),
                        canonical_manifest: "{}".into(),
                        created_at_unix_seconds: 0,
                    },
                    items: vec![RegisteredItem {
                        item_id,
                        title: "Track".into(),
                    }],
                    taxonomy: vec![],
                    generated_local_evidence: None,
                })
                .unwrap();
        }
        repository
            .save_item_feedback(
                "aria-track-v1",
                Activity::DeepWork,
                TrackFeedback::HelpsFocus,
            )
            .unwrap();
        repository.connection.execute(
            "INSERT INTO session_history(id,activity,intensity,session_type,started_at,ended_at,end_reason,focus_seconds) VALUES(?1,'deep_work','medium',?2,10,20,'stopped',10)",
            rusqlite::params!["a".repeat(32), r#"{"kind":"countdown","duration_seconds":600}"#],
        ).unwrap();

        assert_eq!(
            repository
                .rebase_installed_pack_paths(&legacy, &current)
                .unwrap(),
            2
        );
        assert_eq!(
            repository
                .rebase_installed_pack_paths(&legacy, &current)
                .unwrap(),
            0
        );
        assert!(repository
            .list_installed_packs()
            .unwrap()
            .iter()
            .all(|record| Path::new(&record.install_path).starts_with(&current)));
        assert_eq!(
            repository
                .load_item_feedback(Activity::DeepWork, &["aria-track-v1".into()])
                .unwrap()
                .get("aria-track-v1"),
            Some(&TrackFeedback::HelpsFocus)
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM session_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn brand_path_rebase_rejects_outside_paths_without_partial_updates() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("com.adhdmusic.desktop");
        let current = temp.path().join("com.ariazanganeh.ariafocus");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let mut repository = PreferencesRepository::in_memory().unwrap();
        for (pack_id, base, digit) in [("aria.safe", &legacy, '1'), ("aria.outside", &outside, '2')]
        {
            let relative = PathBuf::from("content")
                .join("packs")
                .join(pack_id)
                .join(digit.to_string().repeat(64));
            if base == &legacy {
                std::fs::create_dir_all(current.join(&relative)).unwrap();
            } else {
                std::fs::create_dir_all(base.join(&relative)).unwrap();
            }
            repository
                .register_pack(&PackRegistration {
                    pack: InstalledPackRecord {
                        pack_id: pack_id.into(),
                        title: "Library".into(),
                        version: "1.0.0".into(),
                        manifest_sha256: "a".repeat(64),
                        archive_sha256: "b".repeat(64),
                        install_path: base.join(relative).to_string_lossy().into_owned(),
                        item_count: 1,
                        status: "owner_waived_bundled_private_beta".into(),
                        canonical_manifest: "{}".into(),
                        created_at_unix_seconds: 0,
                    },
                    items: vec![RegisteredItem {
                        item_id: format!("{pack_id}.track"),
                        title: "Track".into(),
                    }],
                    taxonomy: vec![],
                    generated_local_evidence: None,
                })
                .unwrap();
        }

        assert!(repository
            .rebase_installed_pack_paths(&legacy, &current)
            .is_err());
        assert!(Path::new(
            &repository
                .find_installed_pack("aria.safe")
                .unwrap()
                .unwrap()
                .install_path
        )
        .starts_with(&legacy));
    }

    #[test]
    fn studio_jobs_round_trip_and_reject_duplicate_ids() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let job = studio_job('a', 10);
        repository.create_studio_job(&job).unwrap();
        assert_eq!(
            repository.load_studio_job(&job.job_id).unwrap(),
            Some(job.clone())
        );
        assert!(matches!(
            repository.create_studio_job(&job),
            Err(PersistenceError::DuplicateStudioJob(_))
        ));
        let mut duplicate_attempt = studio_job('b', 11);
        duplicate_attempt.attempt_id = job.attempt_id.clone();
        assert!(matches!(
            repository.create_studio_job(&duplicate_attempt),
            Err(PersistenceError::DuplicateStudioAttempt(_))
        ));
    }

    #[test]
    fn studio_jobs_are_recent_first_and_transition_is_atomic() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let first = studio_job('a', 10);
        let second = studio_job('b', 10);
        let third = studio_job('c', 10);
        repository.create_studio_job(&first).unwrap();
        repository.create_studio_job(&second).unwrap();
        repository.create_studio_job(&third).unwrap();
        assert_eq!(
            repository.recent_studio_jobs(10).unwrap(),
            vec![third.clone(), second.clone(), first.clone()]
        );
        assert_eq!(
            repository.recent_studio_jobs(2).unwrap(),
            vec![third.clone(), second.clone()]
        );
        assert_eq!(
            repository
                .recent_studio_jobs(MAX_RECENT_STUDIO_JOBS)
                .unwrap(),
            vec![third.clone(), second.clone(), first.clone()]
        );
        assert!(matches!(
            repository.recent_studio_jobs(0),
            Err(PersistenceError::InvalidStudioJobUpdate)
        ));
        assert!(matches!(
            repository.recent_studio_jobs(MAX_RECENT_STUDIO_JOBS + 1),
            Err(PersistenceError::InvalidStudioJobUpdate)
        ));
        assert!(matches!(
            repository.transition_studio_job(
                &first.job_id,
                1,
                StudioJobState::Generating,
                11,
                None
            ),
            Err(PersistenceError::StaleStudioJob(_))
        ));
        assert_eq!(
            repository.load_studio_job(&first.job_id).unwrap(),
            Some(first.clone())
        );
        assert!(matches!(
            repository.transition_studio_job(&first.job_id, 0, StudioJobState::Ready, 11, None),
            Err(PersistenceError::InvalidStudioJobUpdate)
        ));
        assert_eq!(
            repository.load_studio_job(&first.job_id).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn studio_job_load_rejects_corrupt_or_semantically_invalid_json() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let job = studio_job('a', 10);
        repository.create_studio_job(&job).unwrap();
        repository
            .connection
            .execute(
                "UPDATE music_studio_jobs SET request_json = '{}' WHERE job_id = ?1",
                [job.job_id.as_str()],
            )
            .unwrap();
        assert!(matches!(
            repository.load_studio_job(&job.job_id),
            Err(PersistenceError::InvalidStudioJob)
        ));
        repository.connection.execute("UPDATE music_studio_jobs SET request_json = ?2, prompt_json = '{\"template_version\":1,\"creative_prompt\":\"instrumental but changed\",\"locked_negative_prompt\":\"no vocals, no lyrics, no narration, no speech, no voice, no chanting, no rap, no dialogue, no voiceover, no spoken word\",\"duration_seconds\":180,\"seed\":42}' WHERE job_id = ?1", rusqlite::params![job.job_id.as_str(), studio_json(&job.request).unwrap()]).unwrap();
        assert!(matches!(
            repository.load_studio_job(&job.job_id),
            Err(PersistenceError::InvalidStudioJob)
        ));
    }

    #[test]
    fn studio_job_recovery_is_idempotent_and_preserves_nonrecoverable_states() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let generating = studio_job('a', 10);
        let analyzing = studio_job('b', 20);
        let saving = studio_job('c', 30);
        let ready = studio_job('d', 40);
        let failed = studio_job('e', 50);
        let saved = studio_job('f', 60);
        repository.create_studio_job(&generating).unwrap();
        repository.create_studio_job(&analyzing).unwrap();
        repository.create_studio_job(&saving).unwrap();
        repository.create_studio_job(&ready).unwrap();
        repository.create_studio_job(&failed).unwrap();
        repository.create_studio_job(&saved).unwrap();
        repository
            .transition_studio_job(&generating.job_id, 0, StudioJobState::Generating, 11, None)
            .unwrap();
        repository
            .transition_studio_job(&analyzing.job_id, 0, StudioJobState::Generating, 21, None)
            .unwrap();
        repository
            .transition_studio_job(&analyzing.job_id, 1, StudioJobState::Analyzing, 22, None)
            .unwrap();
        repository
            .transition_studio_job(&saving.job_id, 0, StudioJobState::Generating, 31, None)
            .unwrap();
        repository
            .transition_studio_job(&saving.job_id, 1, StudioJobState::Analyzing, 32, None)
            .unwrap();
        repository
            .transition_studio_job(&saving.job_id, 2, StudioJobState::Ready, 33, None)
            .unwrap();
        repository
            .transition_studio_job(&saving.job_id, 3, StudioJobState::Saving, 34, None)
            .unwrap();
        repository
            .transition_studio_job(&ready.job_id, 0, StudioJobState::Generating, 41, None)
            .unwrap();
        repository
            .transition_studio_job(&ready.job_id, 1, StudioJobState::Analyzing, 42, None)
            .unwrap();
        repository
            .transition_studio_job(&ready.job_id, 2, StudioJobState::Ready, 43, None)
            .unwrap();
        repository
            .transition_studio_job(&failed.job_id, 0, StudioJobState::Generating, 51, None)
            .unwrap();
        repository
            .transition_studio_job(&failed.job_id, 1, StudioJobState::Failed, 52, None)
            .unwrap();
        repository
            .transition_studio_job(&saved.job_id, 0, StudioJobState::Generating, 61, None)
            .unwrap();
        repository
            .transition_studio_job(&saved.job_id, 1, StudioJobState::Analyzing, 62, None)
            .unwrap();
        repository
            .transition_studio_job(&saved.job_id, 2, StudioJobState::Ready, 63, None)
            .unwrap();
        repository
            .transition_studio_job(&saved.job_id, 3, StudioJobState::Saving, 64, None)
            .unwrap();
        repository
            .transition_studio_job(&saved.job_id, 4, StudioJobState::Saved, 65, None)
            .unwrap();
        let failed_before = repository.load_studio_job(&failed.job_id).unwrap().unwrap();
        let saved_before = repository.load_studio_job(&saved.job_id).unwrap().unwrap();
        repository.recover_studio_jobs(100).unwrap();
        assert_eq!(
            repository
                .load_studio_job(&generating.job_id)
                .unwrap()
                .unwrap()
                .state,
            StudioJobState::Interrupted
        );
        assert_eq!(
            repository
                .load_studio_job(&analyzing.job_id)
                .unwrap()
                .unwrap()
                .state,
            StudioJobState::Interrupted
        );
        assert_eq!(
            repository
                .load_studio_job(&saving.job_id)
                .unwrap()
                .unwrap()
                .state,
            StudioJobState::Ready
        );
        let ready_after = repository.load_studio_job(&ready.job_id).unwrap().unwrap();
        assert_eq!(ready_after.state, StudioJobState::Ready);
        assert_eq!(
            repository.load_studio_job(&failed.job_id).unwrap().unwrap(),
            failed_before
        );
        assert_eq!(
            repository.load_studio_job(&saved.job_id).unwrap().unwrap(),
            saved_before
        );
        repository.recover_studio_jobs(101).unwrap();
        assert_eq!(
            repository.load_studio_job(&ready.job_id).unwrap().unwrap(),
            ready_after
        );
        assert_eq!(
            repository.load_studio_job(&failed.job_id).unwrap().unwrap(),
            failed_before
        );
        assert_eq!(
            repository.load_studio_job(&saved.job_id).unwrap().unwrap(),
            saved_before
        );
    }

    #[test]
    fn migration_upgrades_v13_and_artifact_round_trips() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);").unwrap();
        for (version, name, sql) in MIGRATIONS.iter().take(13) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations(version,name) VALUES(?1,?2)",
                    rusqlite::params![version, name],
                )
                .unwrap();
        }
        let job = studio_job('v', 100);
        connection.execute(
            "INSERT INTO music_studio_jobs(job_id,attempt_id,request_json,prompt_json,state,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,'queued',0,?5,?5)",
            rusqlite::params![job.job_id.as_str(), job.attempt_id.as_str(), studio_json(&job.request).unwrap(), studio_json(&job.prompt).unwrap(), job.created_at_ms],
        ).unwrap();
        let mut repository = PreferencesRepository::from_connection(connection).unwrap();
        assert_eq!(repository.schema_version().unwrap(), 16);
        let artifact = ready_artifact(&job);
        repository.upsert_studio_job_artifact(&artifact).unwrap();
        assert_eq!(
            repository.load_studio_job_artifact(&job.job_id).unwrap(),
            Some(artifact)
        );
    }

    #[test]
    fn artifact_validation_rejects_paths_hashes_stages_errors_and_corrupt_rows() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let job = studio_job('x', 10);
        repository.create_studio_job(&job).unwrap();
        let valid = ready_artifact(&job);
        for invalid in [
            StudioJobArtifact {
                output_relative_path: Some("../draft.flac".into()),
                ..valid.clone()
            },
            StudioJobArtifact {
                output_sha256: Some("A".repeat(64)),
                ..valid.clone()
            },
            StudioJobArtifact {
                stage: "mystery".into(),
                ..valid.clone()
            },
            StudioJobArtifact {
                stage: "failed".into(),
                safe_error_code: Some("raw_stderr".into()),
                output_relative_path: None,
                output_sha256: None,
                analysis_json: None,
                ..valid.clone()
            },
        ] {
            assert!(matches!(
                repository.upsert_studio_job_artifact(&invalid),
                Err(PersistenceError::InvalidStudioJob)
            ));
        }
        repository.upsert_studio_job_artifact(&valid).unwrap();
        repository.connection.execute(
            "UPDATE music_studio_job_artifacts SET output_relative_path='C:/escape.flac' WHERE job_id=?1",
            [job.job_id.as_str()],
        ).unwrap();
        assert!(matches!(
            repository.load_studio_job_artifact(&job.job_id),
            Err(PersistenceError::InvalidStudioJob)
        ));
    }

    #[test]
    fn recovery_interrupts_queued_jobs_as_well_as_running_jobs() {
        let mut repository = PreferencesRepository::in_memory().unwrap();
        let queued = studio_job('q', 10);
        repository.create_studio_job(&queued).unwrap();
        repository.recover_studio_jobs(20).unwrap();
        assert_eq!(
            repository
                .load_studio_job(&queued.job_id)
                .unwrap()
                .unwrap()
                .state,
            StudioJobState::Interrupted
        );
    }
}
