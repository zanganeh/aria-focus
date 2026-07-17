//! Tauri 2 application backend coordinating secure pack selection, bounded
//! installed-media preparation, the domain session, native CPAL audio, and
//! local SQLite preferences transactionally.

mod brand_migration;
mod coordinator;
mod music_studio;
mod pack_service;
mod preview_audio;
mod private_beta;
mod review_service;
mod studio_generation;

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{Manager, RunEvent, State};

use audio_engine::{
    NativeAudioFacade, PlaybackSource, PlaybackSourceKind, Provenance, SourceLabel,
};
use catalogue::manifest::{
    ActivitySuitability, AudioAsset, ContentItem, ContentVariant, GeneratorMetadata, HumanQa,
    HumanQaStatus, PackMetadata, Provenance as ManifestProvenance, SafeRegion, SafeRegionKind,
    StimulationAvailability, Taxonomy, TechnicalAnalysis,
};
use catalogue::{ContentPackManifest, GeneratedLocalRecord, LocalGenerationEvidence};
use coordinator::SessionAudioCoordinator;
use domain::{Activity, Intensity, MasterVolume, SessionSnapshot, TrackEnjoyment, TrackFeedback};
use pack_service::{
    ActivityGenreState, ActivityMoodState, FavoriteLibraryItem, ItemFeedbackState, PackService,
    PackSummary,
};
use persistence::{
    GeneratedLocalCustomerRecord, PreferencesRepository, SessionFocusOutcome, SessionHistoryRecord,
    SessionSoundEnjoyment, StudioJobArtifact, StudioJobStore,
};
use preview_audio::{stop_for_focus_start, DraftPreviewState, PreviewAudioCoordinator};
use review_service::{ReviewCandidate, ReviewService};

type Core = SessionAudioCoordinator<NativeAudioFacade, PreferencesRepository>;
type Packs = PackService<PreferencesRepository>;
type Preview = PreviewAudioCoordinator<NativeAudioFacade>;

/// Application state. The session is the single source of truth for transport
/// and elapsed time; the clock base makes `now` a monotonic seconds counter.
struct AppState {
    recovery: RecoverySlots<Core, Packs>,
    review: ReviewService,
    studio_paths: music_studio::StudioRuntimePaths,
    studio_installer: music_studio::RuntimeInstaller,
    studio_jobs: Mutex<Result<PreferencesRepository, String>>,
    studio_generation: Arc<Mutex<()>>,
    studio_active: Arc<AtomicBool>,
    studio_generation_service: Arc<studio_generation::GenerationService>,
    preview: Mutex<Preview>,
    clock_base: std::time::Instant,
    migration: Mutex<brand_migration::BrandMigrationState>,
}

const RECENT_STUDIO_JOB_LIMIT: usize = 12;

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
struct StudioJobSummaryDto {
    id: String,
    status: String,
    updated_at_ms: u64,
    length_seconds: u16,
    stage: String,
    can_preview: bool,
    can_save: bool,
    can_discard: bool,
    safe_message: Option<String>,
}

impl StudioJobSummaryDto {
    fn from_record(record: music_studio_domain::StudioJobRecord) -> Self {
        Self {
            id: record.job_id.as_str().to_owned(),
            status: studio_job_status(record.state).to_owned(),
            updated_at_ms: record.updated_at_ms,
            length_seconds: record.request.duration.seconds(),
            stage: match record.state {
                music_studio_domain::StudioJobState::Queued => "preparing",
                music_studio_domain::StudioJobState::Generating => "creating",
                music_studio_domain::StudioJobState::Analyzing => "checking",
                music_studio_domain::StudioJobState::Ready => "ready",
                _ => "complete",
            }
            .into(),
            can_preview: record.state == music_studio_domain::StudioJobState::Ready,
            can_save: record.state == music_studio_domain::StudioJobState::Ready,
            can_discard: matches!(
                record.state,
                music_studio_domain::StudioJobState::Ready
                    | music_studio_domain::StudioJobState::Queued
                    | music_studio_domain::StudioJobState::Generating
                    | music_studio_domain::StudioJobState::Analyzing
            ),
            safe_message: record
                .failure
                .map(|_| "This music could not be created. Please try again.".into()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateStudioMusicDto {
    activity: Activity,
    focus_type: Activity,
    sound_style_id: String,
    energy: music_studio_domain::StudioEnergy,
    duration: music_studio_domain::StudioDuration,
    note: Option<String>,
    parent_job_id: Option<String>,
}

static STUDIO_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
fn next_studio_id(prefix: &str) -> String {
    format!(
        "{prefix}_{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        STUDIO_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
fn studio_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[allow(dead_code)]
fn create_studio_music_legacy(
    request: CreateStudioMusicDto,
    state: State<AppState>,
) -> Result<StudioJobSummaryDto, String> {
    if request.activity != request.focus_type
        || request
            .note
            .as_ref()
            .is_some_and(|n| n.chars().count() > 240)
    {
        return Err("Please check your music choices and try again.".into());
    }
    let input = music_studio_domain::StudioPromptInput::new(
        request.activity,
        music_studio_domain::StudioId::new(request.sound_style_id)
            .map_err(|_| "Please choose a sound style.".to_owned())?,
        None,
        request.energy,
        vec![],
        request.note,
        None,
        request.duration,
    )
    .map_err(|_| "Please check your music choices and try again.".to_owned())?;
    if state
        .studio_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(
            "Music is already being created. You can make another one when it is finished.".into(),
        );
    }
    let _one_at_a_time = match state.studio_generation.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            state.studio_active.store(false, Ordering::SeqCst);
            return Err(
                "Music is already being created. You can make another one when it is finished."
                    .into(),
            );
        }
    };
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    if store
        .recent_studio_jobs(RECENT_STUDIO_JOB_LIMIT)
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?
        .iter()
        .any(|j| {
            matches!(
                j.state,
                music_studio_domain::StudioJobState::Queued
                    | music_studio_domain::StudioJobState::Generating
                    | music_studio_domain::StudioJobState::Analyzing
            )
        })
    {
        return Err(
            "Music is already being created. You can make another one when it is finished.".into(),
        );
    }
    let parent = request
        .parent_job_id
        .map(music_studio_domain::StudioJobId::new)
        .transpose()
        .map_err(|_| "That music could not be found.".to_owned())?;
    if let Some(parent_id) = &parent {
        if store
            .load_studio_job(parent_id)
            .map_err(|_| "That music could not be found.".to_owned())?
            .is_none()
        {
            state.studio_active.store(false, Ordering::SeqCst);
            return Err("That music could not be found.".into());
        }
    }
    let now = studio_now_ms();
    let seed = now ^ STUDIO_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let record = music_studio_domain::StudioJobRecord {
        job_id: music_studio_domain::StudioJobId::new(next_studio_id("job"))
            .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?,
        attempt_id: music_studio_domain::StudioAttemptId::new(next_studio_id("attempt"))
            .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?,
        prompt: music_studio_domain::build_studio_prompt(&input, seed)
            .map_err(|_| "Please check your music choices and try again.".to_owned())?,
        request: input,
        state: music_studio_domain::StudioJobState::Queued,
        revision: 0,
        created_at_ms: now,
        updated_at_ms: now,
        failure: None,
    };
    if store.create_studio_job(&record).is_err() {
        state.studio_active.store(false, Ordering::SeqCst);
        return Err("Music Studio is temporarily unavailable.".into());
    }
    let paths = state.studio_paths.clone();
    let active = state.studio_active.clone();
    let job = record.clone();
    std::thread::spawn(move || {
        run_studio_job(paths, job, parent);
        active.store(false, Ordering::SeqCst);
    });
    Ok(StudioJobSummaryDto::from_record(record))
}

fn run_studio_job(
    paths: music_studio::StudioRuntimePaths,
    record: music_studio_domain::StudioJobRecord,
    parent_job_id: Option<music_studio_domain::StudioJobId>,
) {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let database = paths
        .resources_dir
        .parent()
        .map(|p| p.join("preferences.sqlite3"));
    let Some(database) = database else { return };
    let Ok(mut store) = PreferencesRepository::open(database) else {
        return;
    };
    let fail = |store: &mut PreferencesRepository,
                record: &music_studio_domain::StudioJobRecord,
                code: &str| {
        let _ = store.transition_studio_job(
            &record.job_id,
            record.revision,
            music_studio_domain::StudioJobState::Failed,
            studio_now_ms(),
            music_studio_domain::StudioFailureDetails::new(
                music_studio_domain::StudioErrorCode::InvalidRequest,
                code.into(),
            )
            .ok(),
        );
    };
    let Ok(generating) = store.transition_studio_job(
        &record.job_id,
        record.revision,
        music_studio_domain::StudioJobState::Generating,
        studio_now_ms(),
        None,
    ) else {
        return;
    };
    let stage = paths
        .resources_dir
        .join("studio-staging")
        .join(record.job_id.as_str());
    if stage.exists() || fs::create_dir_all(stage.parent().unwrap_or(&paths.resources_dir)).is_err()
    {
        fail(&mut store, &generating, "output_invalid");
        return;
    }
    let request_path = stage.with_extension("request.json");
    let request = serde_json::json!({"positive_prompt": record.prompt.creative_prompt, "negative_prompt": record.prompt.locked_negative_prompt, "duration_seconds":record.prompt.duration_seconds, "seed":record.prompt.seed});
    if fs::write(
        &request_path,
        serde_json::to_vec(&request).unwrap_or_default(),
    )
    .is_err()
    {
        fail(&mut store, &generating, "output_invalid");
        return;
    }
    let runtime = paths.installed.join("runtime");
    let python = if cfg!(windows) {
        runtime.join(".venv/Scripts/python.exe")
    } else {
        runtime.join(".venv/bin/python")
    };
    let output = Command::new(python)
        .arg(runtime.join("studio_worker.py"))
        .arg("--request")
        .arg(&request_path)
        .arg("--output-dir")
        .arg(&stage)
        .stdin(Stdio::null())
        // The worker result is its exit status; generation logs may contain
        // model/runtime detail and are intentionally retained nowhere.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = fs::remove_file(&request_path);
    if !matches!(output, Ok(ref status) if status.success()) {
        let _ = fs::remove_dir_all(&stage);
        fail(&mut store, &generating, "generation_failed");
        return;
    }
    let draft = stage.join("draft.flac");
    let valid_regular = fs::symlink_metadata(&draft)
        .map(|m| m.file_type().is_file() && !m.file_type().is_symlink())
        .unwrap_or(false);
    if !valid_regular {
        let _ = fs::remove_dir_all(&stage);
        fail(&mut store, &generating, "output_invalid");
        return;
    }
    let Ok(analyzing) = store.transition_studio_job(
        &record.job_id,
        generating.revision,
        music_studio_domain::StudioJobState::Analyzing,
        studio_now_ms(),
        None,
    ) else {
        return;
    };
    let report = audio_analyzer::analyze_file(&draft);
    let good = report.decode.status == audio_analyzer::DecodeStatus::Decoded
        && report.decode.codec == Some(audio_analyzer::Codec::Flac)
        && report
            .decode
            .duration_seconds
            .is_some_and(|d| (d - f64::from(record.prompt.duration_seconds)).abs() <= 1.0)
        && !report.has_hard_rejections();
    if !good {
        let _ = fs::remove_dir_all(&stage);
        let _ = store.transition_studio_job(
            &record.job_id,
            analyzing.revision,
            music_studio_domain::StudioJobState::Rejected,
            studio_now_ms(),
            None,
        );
        return;
    }
    let target = paths.draft_output_path(&record.job_id);
    if fs::create_dir_all(target.parent().unwrap_or(&paths.resources_dir)).is_err()
        || target.exists()
        || fs::rename(&draft, &target).is_err()
    {
        let _ = fs::remove_dir_all(&stage);
        fail(&mut store, &analyzing, "output_invalid");
        return;
    }
    let analysis = serde_json::to_string(&report).ok();
    let hash = sha256_file(&target).ok();
    let _ = store.upsert_studio_job_artifact(&persistence::StudioJobArtifact {
        job_id: record.job_id.clone(),
        parent_job_id,
        runtime_version: "installed".into(),
        stage: "ready".into(),
        output_relative_path: Some(format!("drafts/{}.flac", record.job_id.as_str())),
        output_sha256: hash,
        analysis_json: analysis,
        safe_error_code: None,
        created_at_ms: record.created_at_ms,
        updated_at_ms: studio_now_ms(),
    });
    let _ = store.transition_studio_job(
        &record.job_id,
        analyzing.revision,
        music_studio_domain::StudioJobState::Ready,
        studio_now_ms(),
        None,
    );
    let _ = fs::remove_dir_all(&stage);
}
fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[allow(dead_code)]
fn cancel_studio_music_legacy(
    job_id: String,
    state: State<AppState>,
) -> Result<StudioJobSummaryDto, String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That music could not be found.".to_owned())?;
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let current = store
        .load_studio_job(&id)
        .map_err(|_| "That music could not be found.".to_owned())?
        .ok_or_else(|| "That music could not be found.".to_owned())?;
    let updated = match current.state {
        music_studio_domain::StudioJobState::Queued
        | music_studio_domain::StudioJobState::Generating => store
            .transition_studio_job(
                &id,
                current.revision,
                music_studio_domain::StudioJobState::Cancelled,
                studio_now_ms(),
                None,
            )
            .map_err(|_| "Music could not be cancelled right now.".to_owned())?,
        _ => current,
    };
    Ok(StudioJobSummaryDto::from_record(updated))
}

#[tauri::command]
fn create_studio_music(
    request: studio_generation::CreateStudioMusicRequest,
    state: State<AppState>,
) -> Result<StudioJobSummaryDto, String> {
    state
        .studio_generation_service
        .create(request)
        .map(StudioJobSummaryDto::from_record)
}

#[tauri::command]
fn cancel_studio_music(
    job_id: String,
    state: State<AppState>,
) -> Result<StudioJobSummaryDto, String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That music could not be found.".to_owned())?;
    state
        .studio_generation_service
        .cancel(&id)
        .map(StudioJobSummaryDto::from_record)
}

fn studio_job_status(state: music_studio_domain::StudioJobState) -> &'static str {
    use music_studio_domain::StudioJobState::*;
    match state {
        Ready => "Ready",
        Saved => "Saved",
        Queued | Generating | Analyzing | Saving => "In progress",
        Rejected | Failed | Cancelled | Interrupted => "Needs attention",
    }
}

fn map_recent_studio_jobs(
    result: Result<Vec<music_studio_domain::StudioJobRecord>, persistence::PersistenceError>,
) -> Result<Vec<StudioJobSummaryDto>, String> {
    result
        .map(|records| {
            records
                .into_iter()
                .map(StudioJobSummaryDto::from_record)
                .collect()
        })
        .map_err(|_| "Saved Music Studio items could not be read right now.".to_owned())
}

/// Startup services are recovered independently. Candidates are created
/// outside locks and only committed when the corresponding slot still failed.
struct RecoverySlots<C, P> {
    core: Mutex<Result<C, String>>,
    packs: Mutex<Result<P, String>>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct StartupHealth {
    core_ready: bool,
    core_error: Option<String>,
    packs_ready: bool,
    packs_error: Option<String>,
    migration_status: brand_migration::BrandMigrationStatus,
    migration_error: Option<String>,
}

impl<C, P> RecoverySlots<C, P> {
    fn health(&self) -> StartupHealth {
        let core = self.core.lock().expect("startup core mutex poisoned");
        let (core_ready, core_error) = match &*core {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.clone())),
        };
        drop(core);
        let packs = self.packs.lock().expect("startup packs mutex poisoned");
        let (packs_ready, packs_error) = match &*packs {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.clone())),
        };
        StartupHealth {
            core_ready,
            core_error,
            packs_ready,
            packs_error,
            migration_status: brand_migration::BrandMigrationStatus::NotNeeded,
            migration_error: None,
        }
    }

    fn retry_failed_with(
        &self,
        app_data_dir: Result<PathBuf, String>,
        initialize_core: impl FnOnce(&Path) -> Result<C, String>,
        initialize_packs: impl FnOnce(&Path) -> Result<P, String>,
    ) -> StartupHealth {
        let retry_core = self.core.lock().map(|slot| slot.is_err()).unwrap_or(false);
        let retry_packs = self.packs.lock().map(|slot| slot.is_err()).unwrap_or(false);
        match app_data_dir {
            Ok(path) => {
                if retry_core {
                    self.commit_core_if_failed(initialize_core(&path));
                }
                if retry_packs {
                    self.commit_packs_if_failed(initialize_packs(&path));
                }
            }
            Err(error) => {
                if retry_core {
                    self.commit_core_if_failed(Err(error.clone()));
                }
                if retry_packs {
                    self.commit_packs_if_failed(Err(error));
                }
            }
        }
        self.health()
    }

    fn commit_core_if_failed(&self, candidate: Result<C, String>) {
        if let Ok(mut slot) = self.core.lock() {
            if slot.is_err() {
                *slot = candidate;
            }
        }
    }

    fn commit_packs_if_failed(&self, candidate: Result<P, String>) {
        if let Ok(mut slot) = self.packs.lock() {
            if slot.is_err() {
                *slot = candidate;
            }
        }
    }
}

impl AppState {
    fn startup_health(&self) -> StartupHealth {
        let mut health = self.recovery.health();
        if let Ok(migration) = self.migration.lock() {
            health.migration_status = migration.status;
            health.migration_error = migration.error.clone();
        }
        health
    }

    fn now_secs(&self) -> u64 {
        self.clock_base.elapsed().as_secs()
    }

    fn wall_clock_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn with_core<T>(
        &self,
        operation: impl FnOnce(&mut Core) -> Result<T, coordinator::CoordinatorError>,
    ) -> Result<T, String> {
        let mut guard = self
            .recovery
            .core
            .lock()
            .map_err(|error| error.to_string())?;
        let core = guard.as_mut().map_err(|error| error.clone())?;
        operation(core).map_err(|error| error.to_string())
    }

    fn with_packs<T>(
        &self,
        operation: impl FnOnce(&mut Packs) -> Result<T, pack_service::PackServiceError>,
    ) -> Result<T, String> {
        let mut guard = self
            .recovery
            .packs
            .lock()
            .map_err(|error| error.to_string())?;
        let packs = guard.as_mut().map_err(|error| error.clone())?;
        operation(packs).map_err(|error| error.to_string())
    }

    fn stop_preview(&self) -> Result<(), String> {
        let mut preview = self.preview.lock().map_err(|error| error.to_string())?;
        stop_for_focus_start(&mut preview).map_err(|error| error.to_string())
    }
}

fn parse_intensity(s: &str) -> Result<Intensity, String> {
    match s {
        "off" => Ok(Intensity::Off),
        "low" => Ok(Intensity::Low),
        "medium" => Ok(Intensity::Medium),
        "high" => Ok(Intensity::High),
        other => Err(format!("unknown intensity: {other}")),
    }
}

fn parse_activity(s: &str) -> Result<Activity, String> {
    Activity::from_storage_key(s).ok_or_else(|| format!("unknown activity: {s}"))
}

fn parse_track_feedback(s: &str) -> Result<TrackFeedback, String> {
    match s {
        "helps_focus" => Ok(TrackFeedback::HelpsFocus),
        "neutral" => Ok(TrackFeedback::Neutral),
        "distracting" => Ok(TrackFeedback::Distracting),
        _ => Err(
            "Focus feedback must be Helps focus, Neutral, or Distracting. Use Clear to unset it."
                .to_owned(),
        ),
    }
}

fn parse_track_enjoyment(s: &str) -> Result<TrackEnjoyment, String> {
    TrackEnjoyment::from_storage_key(s).ok_or_else(|| {
        "Enjoyment must be Liked or Not for me. Refresh the track and try again.".to_owned()
    })
}

fn parse_session_focus_outcome(value: &str) -> Result<SessionFocusOutcome, String> {
    match value {
        "helped_focus" => Ok(SessionFocusOutcome::HelpedFocus),
        "neutral" => Ok(SessionFocusOutcome::Neutral),
        "distracting" => Ok(SessionFocusOutcome::Distracting),
        _ => Err("Session focus outcome must be Helped focus, Neutral, or Distracting.".into()),
    }
}
fn parse_session_sound_enjoyment(value: &str) -> Result<SessionSoundEnjoyment, String> {
    match value {
        "liked" => Ok(SessionSoundEnjoyment::Liked),
        "not_for_me" => Ok(SessionSoundEnjoyment::NotForMe),
        _ => Err("Session sound enjoyment must be Liked or Not for me.".into()),
    }
}

fn ensure_focus_is_idle_for_draft_preview(status: domain::SessionStatus) -> Result<(), String> {
    if status == domain::SessionStatus::Idle {
        Ok(())
    } else {
        Err("Stop focus playback before previewing a Music Studio draft.".to_owned())
    }
}

#[tauri::command]
fn get_studio_capability(state: State<AppState>) -> music_studio_domain::StudioCapability {
    music_studio::detect_capability(&state.studio_paths)
}

#[tauri::command]
fn get_runtime_install(state: State<AppState>) -> music_studio::RuntimeInstallDto {
    state.studio_installer.state()
}

#[tauri::command]
fn cancel_runtime_install(state: State<AppState>) -> music_studio::RuntimeInstallDto {
    state.studio_installer.cancel();
    state.studio_installer.state()
}

#[tauri::command]
fn start_runtime_install(
    state: State<AppState>,
) -> Result<music_studio::RuntimeInstallDto, String> {
    state.studio_installer.start(false)
}

#[tauri::command]
fn repair_runtime(state: State<AppState>) -> Result<music_studio::RuntimeInstallDto, String> {
    state.studio_installer.start(true)
}

#[tauri::command]
fn list_recent_studio_jobs(state: State<AppState>) -> Result<Vec<StudioJobSummaryDto>, String> {
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Saved Music Studio items are temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Saved Music Studio items are temporarily unavailable.".to_owned())?;
    map_recent_studio_jobs(store.recent_studio_jobs(RECENT_STUDIO_JOB_LIMIT))
}

#[tauri::command]
fn get_studio_job(
    job_id: String,
    state: State<AppState>,
) -> Result<Option<StudioJobSummaryDto>, String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That saved Music Studio item could not be found.".to_owned())?;
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Saved Music Studio items are temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Saved Music Studio items are temporarily unavailable.".to_owned())?;
    store
        .load_studio_job(&id)
        .map(|record| record.map(StudioJobSummaryDto::from_record))
        .map_err(|_| "That saved Music Studio item could not be read right now.".to_owned())
}

#[tauri::command]
fn start_session(state: State<AppState>) -> Result<(), String> {
    state.stop_preview()?;
    let now = state.now_secs();
    // Keep activity, fully revalidated selection, decode, native preparation,
    // and recent-history commit within one lock order and command outcome.
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let activity = core.snapshot(now).activity;
    let genre_state = packs
        .genre_state(activity)
        .map_err(|error| error.to_string())?;
    if genre_state.selected_genre_id.is_some() && !genre_state.selected_genre_available {
        return Err("The saved genre is no longer available for this activity. Choose Any compatible genre or another available genre before starting.".to_owned());
    }
    let mood_state = packs
        .mood_state(activity, genre_state.selected_genre_id.as_deref())
        .map_err(|error| error.to_string())?;
    if mood_state.selected_mood_id.is_some() && !mood_state.selected_mood_available {
        return Err("The saved mood is no longer available for this activity and genre. Choose Any compatible mood or another available mood before starting.".to_owned());
    }
    let prepared = packs
        .prepare_playback(
            activity,
            genre_state.selected_genre_id.as_deref(),
            mood_state.selected_mood_id.as_deref(),
        )
        .map_err(|error| error.to_string())?;
    let (source, recent) = match prepared {
        Some(prepared) => (
            PlaybackSource::Installed(prepared.program),
            Some(prepared.primary_item_id),
        ),
        // The minimal home screen shows every activity as immediately startable. Until an
        // activity has an installed authored track, its default Any/Any choice uses the
        // explicit local procedural fallback rather than presenting a start failure.
        None if genre_state.selected_genre_id.is_none() && mood_state.selected_mood_id.is_none() => (PlaybackSource::TestTone, None),
        None => return Err("No eligible installed track is available for this activity, genre, and mood. Choose another preference or update track feedback.".to_owned()),
    };
    core.start_recorded(now, state.wall_clock_secs(), source)
        .map_err(|error| error.to_string())?;
    if let Some(item_id) = recent {
        packs.commit_playback(item_id);
    }
    Ok(())
}

#[tauri::command]
fn list_favorites(state: State<AppState>) -> Result<Vec<FavoriteLibraryItem>, String> {
    state.with_packs(|packs| packs.favorites())
}

#[tauri::command]
fn list_my_music(state: State<AppState>) -> Result<Vec<pack_service::MyMusicItem>, String> {
    state.with_packs(|packs| packs.list_my_music())
}

#[tauri::command]
fn rename_my_music(item_id: String, title: String, state: State<AppState>) -> Result<(), String> {
    let title = customer_title(&title)?;
    state.with_packs(|packs| packs.rename_my_music(&item_id, &title))
}

#[tauri::command]
fn delete_my_music(item_id: String, state: State<AppState>) -> Result<(), String> {
    let now = state.now_secs();
    let preview = state.preview.lock().map_err(|error| error.to_string())?;
    if preview
        .current_job_id()
        .is_some_and(|job| item_id == format!("generated.local.{job}.item"))
    {
        return Err("Stop the preview before deleting this music.".into());
    }
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    if core.source_kind() == PlaybackSourceKind::Installed
        && core.source_label().item_id == item_id
        && session_holds_live_audio(core.snapshot(now).status)
    {
        return Err(domain::SessionError::AlreadyActive.to_string());
    }
    packs
        .delete_my_music(&item_id)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_my_music(
    item_id: String,
    activity: Activity,
    state: State<AppState>,
) -> Result<(), String> {
    state.stop_preview()?;
    let now = state.now_secs();
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    if core.snapshot(now).status != domain::SessionStatus::Idle {
        return Err("Stop the current session before playing this music.".into());
    }
    let prepared = packs
        .prepare_my_music_playback(activity, &item_id)
        .map_err(|error| error.to_string())?;
    core.start_favorite_with_source(
        now,
        state.wall_clock_secs(),
        activity,
        PlaybackSource::Installed(prepared.program),
    )
    .map_err(|error| error.to_string())?;
    packs.commit_playback(prepared.primary_item_id);
    Ok(())
}

#[tauri::command]
fn discard_studio_draft(job_id: String, state: State<AppState>) -> Result<(), String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That draft could not be found.".to_owned())?;
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let job = store
        .load_studio_job(&id)
        .map_err(|_| "That draft could not be read.".to_owned())?
        .ok_or_else(|| "That draft could not be found.".to_owned())?;
    if !matches!(
        job.state,
        music_studio_domain::StudioJobState::Ready
            | music_studio_domain::StudioJobState::Rejected
            | music_studio_domain::StudioJobState::Failed
            | music_studio_domain::StudioJobState::Cancelled
            | music_studio_domain::StudioJobState::Interrupted
    ) {
        return Err(
            "This music cannot be discarded while it is being created or has been saved.".into(),
        );
    }
    state.stop_preview()?;
    let draft = state.studio_paths.draft_output_path(&id);
    if draft.exists() {
        let meta = std::fs::symlink_metadata(&draft)
            .map_err(|_| "This draft could not be discarded.".to_owned())?;
        if !meta.file_type().is_file() || meta.file_type().is_symlink() {
            return Err("This draft could not be discarded.".into());
        }
    }
    let stage = state.studio_paths.job_output_dir(&id);
    if stage.exists() {
        let meta = std::fs::symlink_metadata(&stage)
            .map_err(|_| "This draft could not be discarded.".to_owned())?;
        if !meta.file_type().is_dir() || meta.file_type().is_symlink() {
            return Err("This draft could not be discarded.".into());
        }
    }
    store
        .remove_studio_job(&id)
        .map_err(|_| "This draft could not be discarded.".to_owned())?;
    // The durable record is removed first so a restart can never reopen a discarded draft.
    // Cleanup is best-effort because an antivirus/file indexer may briefly retain a handle.
    let _ = std::fs::remove_file(&draft);
    let _ = std::fs::remove_dir_all(&stage);
    Ok(())
}

#[tauri::command]
fn remove_favorite(
    item_id: String,
    activity: String,
    state: State<AppState>,
) -> Result<(), String> {
    let activity = parse_activity(&activity)?;
    state.with_packs(|packs| packs.remove_favorite(activity, &item_id))
}

#[tauri::command]
fn start_favorite(item_id: String, activity: String, state: State<AppState>) -> Result<(), String> {
    state.stop_preview()?;
    let activity = parse_activity(&activity)?;
    let now = state.now_secs();
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    if matches!(
        core.snapshot(now).status,
        domain::SessionStatus::Playing | domain::SessionStatus::Paused
    ) {
        return Err("Stop the current session before starting a favourite.".to_owned());
    }
    let prepared = packs
        .prepare_favorite_playback(activity, &item_id)
        .map_err(|error| error.to_string())?;
    core.start_favorite_with_source(
        now,
        state.wall_clock_secs(),
        activity,
        PlaybackSource::Installed(prepared.program),
    )
    .map_err(|error| error.to_string())?;
    packs.commit_playback(prepared.primary_item_id);
    Ok(())
}

#[tauri::command]
fn get_onboarding_preferences(
    state: State<AppState>,
) -> Result<persistence::OnboardingPreferences, String> {
    state.with_core(|core| core.onboarding_preferences())
}

#[tauri::command]
fn complete_onboarding(
    intensity: String,
    genres: Vec<String>,
    state: State<AppState>,
) -> Result<(), String> {
    let intensity = parse_intensity(&intensity)?;
    if !matches!(
        intensity,
        Intensity::Low | Intensity::Medium | Intensity::High
    ) {
        return Err("Choose Low, Medium, or High for a starting stimulation preference.".into());
    }
    if genres.len() > 3 {
        return Err("Choose at most three genres.".into());
    }
    let now = state.now_secs();
    let mut packs_guard = state.recovery.packs.lock().map_err(|e| e.to_string())?;
    let packs = packs_guard.as_mut().map_err(|e| e.clone())?;
    let (source, recent) = match packs.prepare_playback(Activity::DeepWork, None, None).map_err(|e| e.to_string())? {
        Some(prepared) => (
            PlaybackSource::Installed(prepared.program),
            Some(prepared.primary_item_id),
        ),
        None if packs.list().map_err(|e| e.to_string())?.is_empty() => (PlaybackSource::TestTone, None),
        None => return Err("No eligible Deep Work audio is available. Try again after choosing another installed profile.".into()),
    };
    let mut core_guard = state.recovery.core.lock().map_err(|e| e.to_string())?;
    let core = core_guard.as_mut().map_err(|e| e.clone())?;
    core.complete_onboarding(now, state.wall_clock_secs(), intensity, &genres, source)
        .map_err(|e| e.to_string())?;
    if let Some(item_id) = recent {
        packs.commit_playback(item_id);
    }
    Ok(())
}

#[tauri::command]
fn list_review_candidates(state: State<AppState>) -> Vec<ReviewCandidate> {
    if state.review.available() {
        state.review.list()
    } else {
        Vec::new()
    }
}

#[tauri::command]
fn start_review_candidate(review_id: String, state: State<AppState>) -> Result<(), String> {
    let now = state.now_secs();
    let mut core = state.recovery.core.lock().map_err(|e| e.to_string())?;
    let core = core.as_mut().map_err(|e| e.clone())?;
    if core.snapshot(now).status != domain::SessionStatus::Idle {
        return Err(
            "Stop the current session before starting a quarantined review candidate.".to_owned(),
        );
    }
    let program = state.review.prepare(&review_id)?;
    core.start_recorded(
        now,
        state.wall_clock_secs(),
        PlaybackSource::Review(program),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn pause_session(state: State<AppState>) -> Result<(), String> {
    let now = state.now_secs();
    state.with_core(|core| core.pause(now))
}

#[tauri::command]
fn resume_session(state: State<AppState>) -> Result<(), String> {
    let now = state.now_secs();
    state.with_core(|core| core.resume(now))
}

#[tauri::command]
fn stop_session(state: State<AppState>) -> Result<(), String> {
    let now = state.now_secs();
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    core.stop_at(now, state.wall_clock_secs())
        .map_err(|error| error.to_string())?;
    if let Ok(packs) = packs_guard.as_mut() {
        commit_current_item(packs, core);
    }
    Ok(())
}

#[tauri::command]
fn next_track(state: State<AppState>) -> Result<(), String> {
    state.with_core(|core| core.next_track())
}

#[tauri::command]
fn previous_track(state: State<AppState>) -> Result<(), String> {
    state.with_core(|core| core.previous_track())
}

#[tauri::command]
fn set_activity(activity: String, state: State<AppState>) -> Result<(), String> {
    let activity = parse_activity(&activity)?;
    state.with_core(|core| core.select_activity(activity))
}

#[tauri::command]
fn get_activity_genres(state: State<AppState>) -> Result<ActivityGenreState, String> {
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    packs
        .genre_state(core.snapshot(state.now_secs()).activity)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_activity_genre(
    genre_id: Option<String>,
    state: State<AppState>,
) -> Result<ActivityGenreState, String> {
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let snapshot = core.snapshot(state.now_secs());
    if matches!(
        snapshot.status,
        domain::SessionStatus::Playing | domain::SessionStatus::Paused
    ) {
        return Err("Genre cannot be changed while transport is active.".to_owned());
    }
    packs
        .set_genre_preference(snapshot.activity, genre_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_activity_moods(state: State<AppState>) -> Result<ActivityMoodState, String> {
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let activity = core.snapshot(state.now_secs()).activity;
    let genre_id = packs
        .genre_state(activity)
        .map_err(|error| error.to_string())?
        .selected_genre_id;
    packs
        .mood_state(activity, genre_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_activity_mood(
    mood_id: Option<String>,
    state: State<AppState>,
) -> Result<ActivityMoodState, String> {
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let snapshot = core.snapshot(state.now_secs());
    if matches!(
        snapshot.status,
        domain::SessionStatus::Playing | domain::SessionStatus::Paused
    ) {
        return Err("Mood cannot be changed while transport is active.".to_owned());
    }
    let genre_id = packs
        .genre_state(snapshot.activity)
        .map_err(|error| error.to_string())?
        .selected_genre_id;
    packs
        .set_mood_preference(snapshot.activity, genre_id.as_deref(), mood_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_intensity(intensity: String, state: State<AppState>) -> Result<(), String> {
    let i = parse_intensity(&intensity)?;
    state.with_core(|core| core.set_intensity(i))
}

#[tauri::command]
fn get_master_volume(state: State<AppState>) -> Result<u8, String> {
    state.with_core(|core| Ok(core.master_volume().percent()))
}

#[tauri::command]
fn set_master_volume(volume: u8, state: State<AppState>) -> Result<u8, String> {
    let volume = MasterVolume::new(volume).map_err(|error| error.to_string())?;
    let percent = state.with_core(|core| {
        core.set_master_volume(volume)?;
        Ok(core.master_volume().percent())
    })?;
    state
        .preview
        .lock()
        .map_err(|error| error.to_string())?
        .set_master_volume(percent)
        .map_err(|error| error.to_string())?;
    Ok(percent)
}

#[tauri::command]
fn start_draft_preview(job_id: String, state: State<AppState>) -> Result<(), String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That Music Studio draft could not be previewed.".to_owned())?;
    let now = state.now_secs();
    let volume = state
        .with_core(|core| {
            ensure_focus_is_idle_for_draft_preview(core.snapshot(now).status).map_err(|_| {
                coordinator::CoordinatorError::Domain(domain::SessionError::AlreadyActive)
            })?;
            Ok(core.master_volume().percent())
        })
        .map_err(|_| "Stop focus playback before previewing a Music Studio draft.".to_owned())?;
    let path = state.studio_paths.draft_output_path(&id);
    state
        .preview
        .lock()
        .map_err(|error| error.to_string())?
        .start(path, id.as_str(), volume)
        .map_err(|_| {
            "This Music Studio draft is missing, corrupt, or not a playable FLAC file.".to_owned()
        })
}

#[tauri::command]
fn pause_draft_preview(state: State<AppState>) -> Result<(), String> {
    state
        .preview
        .lock()
        .map_err(|error| error.to_string())?
        .pause()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_draft_preview(state: State<AppState>) -> Result<(), String> {
    state
        .preview
        .lock()
        .map_err(|error| error.to_string())?
        .resume()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn stop_draft_preview(state: State<AppState>) -> Result<(), String> {
    state.stop_preview()
}

#[tauri::command]
fn get_draft_preview_state(state: State<AppState>) -> Result<DraftPreviewState, String> {
    Ok(state
        .preview
        .lock()
        .map_err(|error| error.to_string())?
        .state())
}

fn customer_title(title: &str) -> Result<String, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 100 || trimmed.chars().any(char::is_control)
    {
        return Err("Choose a name between 1 and 100 characters without line breaks.".into());
    }
    Ok(trimmed.into())
}

fn session_holds_live_audio(status: domain::SessionStatus) -> bool {
    matches!(
        status,
        domain::SessionStatus::Playing | domain::SessionStatus::Paused
    )
}

fn reconciled_saved_item(
    items: Vec<pack_service::MyMusicItem>,
    job_id: &str,
    title: &str,
) -> Result<Option<pack_service::MyMusicItem>, String> {
    let Some(item) = items.into_iter().find(|item| item.job_id == job_id) else {
        return Ok(None);
    };
    if item.title != title {
        return Err("This music was already saved with a different name.".into());
    }
    Ok(Some(item))
}

fn ready_generated_record(
    job: &music_studio_domain::StudioJobRecord,
    artifact: &StudioJobArtifact,
    paths: &music_studio::StudioRuntimePaths,
    title: String,
) -> Result<(GeneratedLocalRecord, PathBuf), String> {
    if job.state != music_studio_domain::StudioJobState::Ready
        || artifact.stage != "ready"
        || artifact.output_relative_path.as_deref()
            != Some(format!("drafts/{}.flac", job.job_id.as_str()).as_str())
    {
        return Err("This draft is not ready to save yet.".into());
    }
    let expected = paths.draft_output_path(&job.job_id);
    let metadata = std::fs::symlink_metadata(&expected)
        .map_err(|_| "This draft is no longer available.".to_owned())?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("This draft is no longer available.".into());
    }
    let hash = sha256_file(&expected).map_err(|_| "This draft could not be checked.".to_owned())?;
    if artifact.output_sha256.as_deref() != Some(hash.as_str()) {
        return Err("This draft changed after it was checked. Please generate it again.".into());
    }
    let analysis: serde_json::Value = serde_json::from_str(
        artifact
            .analysis_json
            .as_deref()
            .ok_or_else(|| "This draft is missing its check result.".to_owned())?,
    )
    .map_err(|_| "This draft is missing its check result.".to_owned())?;
    let duration = analysis
        .get("duration_seconds")
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 1.0)
        .ok_or_else(|| "This draft is missing its check result.".to_owned())?
        as f32;
    if analysis.get("codec").and_then(serde_json::Value::as_str) != Some("flac")
        || analysis
            .get("sample_rate_hz")
            .and_then(serde_json::Value::as_u64)
            != Some(48_000)
        || analysis.get("channels").and_then(serde_json::Value::as_u64) != Some(2)
        || analysis
            .get("vocal_speech")
            .and_then(serde_json::Value::as_str)
            != Some("not_assessed")
    {
        return Err("This draft is missing its check result.".into());
    }
    let job_id = job.job_id.as_str();
    let asset_path = format!("assets/generated/{job_id}.flac");
    let manifest = ContentPackManifest {
        format: "adhdpack".into(),
        format_version: 1,
        pack: PackMetadata {
            id: format!("generated.local.{job_id}"),
            title: title.clone(),
            description: "Created on this device.".into(),
            version: "1.0.0".into(),
            app_version_requirement: "*".into(),
        },
        taxonomy: Taxonomy {
            genres: vec![],
            moods: vec![],
        },
        items: vec![ContentItem {
            id: format!("generated.local.{job_id}.item"),
            title,
            genre_ids: vec![],
            mood_ids: vec![],
            activity_suitability: vec![ActivitySuitability {
                activity: job.request.activity,
                suitability: 1.0,
            }],
            provenance: ManifestProvenance {
                source: "generated_local".into(),
                licence_id: "created_on_device".into(),
                licence_url: None,
                composer: None,
                generator: Some(GeneratorMetadata {
                    provider: "Music Studio".into(),
                    model: artifact.runtime_version.clone(),
                    model_version: artifact.runtime_version.clone(),
                    prompt: "Local Music Studio generation".into(),
                }),
                contains_lyrics: false,
                contains_speech: false,
            },
            analysis: TechnicalAnalysis {
                duration_seconds: duration,
                integrated_lufs: -20.0,
                true_peak_dbfs: -3.0,
                loudness_range_lu: 0.0,
                spectral_centroid_hz: 0.0,
                high_frequency_energy_ratio: 0.0,
                onset_density_per_second: 0.0,
                tempo_bpm: 60.0,
                tempo_confidence: 0.0,
                tempo_drift_percent: 0.0,
                section_change_novelty: 0.0,
                unexplained_silence_seconds: 0.0,
                clipped_samples: 0,
                discontinuity_detected: false,
                codec_errors_detected: false,
                corruption_detected: false,
                vocal_speech_likelihood: 0.0,
            },
            variants: vec![ContentVariant {
                id: "original".into(),
                asset: AudioAsset {
                    path: asset_path,
                    sha256: hash,
                    bytes: metadata.len(),
                    codec: catalogue::AssetCodec::Flac,
                    sample_rate_hz: 48_000,
                    channels: 2,
                    bit_depth: None,
                },
                safe_regions: vec![SafeRegion {
                    kind: SafeRegionKind::Loop,
                    start_seconds: 0.0,
                    end_seconds: duration,
                }],
                stimulation_available: vec![
                    StimulationAvailability::Off,
                    StimulationAvailability::Low,
                    StimulationAvailability::Medium,
                    StimulationAvailability::High,
                ],
            }],
            human_qa: HumanQa {
                status: HumanQaStatus::Draft,
                reviews: vec![],
            },
            cover: None,
        }],
    };
    Ok((
        GeneratedLocalRecord {
            generation_job_id: job_id.into(),
            manifest,
            evidence: LocalGenerationEvidence {
                producer: "adhd-music-studio".into(),
                job_id: job_id.into(),
                completed_at_unix_seconds: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            },
        },
        expected,
    ))
}

#[tauri::command]
fn save_studio_draft(
    job_id: String,
    title: String,
    state: State<AppState>,
) -> Result<pack_service::MyMusicItem, String> {
    let id = music_studio_domain::StudioJobId::new(job_id)
        .map_err(|_| "That draft could not be found.".to_owned())?;
    let title = customer_title(&title)?;
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let job = store
        .load_studio_job(&id)
        .map_err(|_| "That draft could not be read.".to_owned())?
        .ok_or_else(|| "That draft could not be found.".to_owned())?;
    if job.state == music_studio_domain::StudioJobState::Saved {
        drop(jobs);
        return state
            .with_packs(|packs| packs.list_my_music())
            .and_then(|items| {
                items
                    .into_iter()
                    .find(|item| item.job_id == id.as_str())
                    .ok_or_else(|| "That saved music could not be found.".into())
            });
    }
    let artifact = store
        .load_studio_job_artifact(&id)
        .map_err(|_| "That draft could not be read.".to_owned())?
        .ok_or_else(|| "This draft is not ready to save yet.".to_owned())?;
    let (record, source) =
        ready_generated_record(&job, &artifact, &state.studio_paths, title.clone())?;
    let saving = store
        .transition_studio_job(
            &id,
            job.revision,
            music_studio_domain::StudioJobState::Saving,
            studio_now_ms(),
            None,
        )
        .map_err(|_| "This draft is not ready to save yet.".to_owned())?;
    drop(jobs);
    state.stop_preview()?;
    let stage = state.with_packs(|packs| packs.generated_local_staging_path(id.as_str()))?;
    let result = (|| {
        if let Some(item) = reconciled_saved_item(
            state.with_packs(|packs| packs.list_my_music())?,
            id.as_str(),
            &title,
        )? {
            return Ok(item);
        }
        if stage.exists() {
            return Err("This draft could not be saved right now.".to_owned());
        }
        std::fs::create_dir_all(stage.join("assets/generated"))
            .map_err(|_| "This draft could not be saved right now.".to_owned())?;
        std::fs::copy(
            &source,
            stage.join(format!("assets/generated/{}.flac", id.as_str())),
        )
        .map_err(|_| "This draft could not be saved right now.".to_owned())?;
        state.with_packs(|packs| {
            packs.install_generated_local(
                record.clone(),
                GeneratedLocalCustomerRecord {
                    pack_id: record.manifest.pack.id.clone(),
                    item_id: record.manifest.items[0].id.clone(),
                    title,
                    activity: job.request.activity,
                    created_at_unix_seconds: record.evidence.completed_at_unix_seconds,
                },
                &stage,
            )?;
            Ok(())
        })?;
        state
            .with_packs(|packs| packs.list_my_music())
            .and_then(|items| {
                items
                    .into_iter()
                    .find(|item| item.job_id == id.as_str())
                    .ok_or_else(|| "This draft could not be saved right now.".into())
            })
    })();
    let mut jobs = state
        .studio_jobs
        .lock()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    let store = jobs
        .as_mut()
        .map_err(|_| "Music Studio is temporarily unavailable.".to_owned())?;
    match result {
        Ok(item) => {
            store
                .transition_studio_job(
                    &id,
                    saving.revision,
                    music_studio_domain::StudioJobState::Saved,
                    studio_now_ms(),
                    None,
                )
                .map_err(|_| "This draft was saved but needs attention.".to_owned())?;
            let _ = std::fs::remove_file(source);
            Ok(item)
        }
        Err(error) => {
            let _ = store.transition_studio_job(
                &id,
                saving.revision,
                music_studio_domain::StudioJobState::Ready,
                studio_now_ms(),
                None,
            );
            let _ = std::fs::remove_dir_all(stage);
            Err(error)
        }
    }
}

#[tauri::command]
fn set_session_type(
    session_type: domain::SessionType,
    state: State<AppState>,
) -> Result<(), String> {
    state.with_core(|core| core.set_session_type(session_type))
}

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Result<SessionSnapshot, String> {
    let now = state.now_secs();
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let snapshot = core
        .tick_at(now, state.wall_clock_secs())
        .map_err(|error| error.to_string())?;
    if snapshot.status == domain::SessionStatus::Expired {
        if let Ok(packs) = packs_guard.as_mut() {
            commit_current_item(packs, core);
        }
    }
    Ok(snapshot)
}

#[tauri::command]
fn list_recent_sessions(state: State<AppState>) -> Result<Vec<SessionHistoryRecord>, String> {
    state.with_core(|core| core.recent_history(10))
}

#[tauri::command]
fn save_session_rating(
    id: String,
    focus_outcome: Option<String>,
    sound_enjoyment: Option<String>,
    state: State<AppState>,
) -> Result<(), String> {
    let focus_outcome = focus_outcome
        .as_deref()
        .map(parse_session_focus_outcome)
        .transpose()?;
    let sound_enjoyment = sound_enjoyment
        .as_deref()
        .map(parse_session_sound_enjoyment)
        .transpose()?;
    state.with_core(|core| core.save_session_ratings(&id, focus_outcome, sound_enjoyment))
}

fn commit_current_item(packs: &mut Packs, core: &Core) {
    let label = core.source_label();
    if matches!(core.source_kind(), PlaybackSourceKind::Installed) {
        packs.commit_playback(label.item_id);
    }
}

#[tauri::command]
fn list_content_packs(state: State<AppState>) -> Result<Vec<PackSummary>, String> {
    state.with_packs(PackService::list)
}

#[tauri::command]
fn import_content_pack(path: String, state: State<AppState>) -> Result<PackSummary, String> {
    state.with_packs(|packs| packs.import(Path::new(&path)))
}

fn app_data_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| {
        format!(
            "Local preferences could not start because the app data directory is unavailable: {error}. Restart the app; if this continues, check your user-profile permissions."
        )
    })
}

fn prepare_app_data(path: &Path) -> Result<brand_migration::BrandMigrationStatus, String> {
    brand_migration::prepare(path)
}

#[tauri::command]
fn get_startup_health(state: State<AppState>) -> StartupHealth {
    state.startup_health()
}

#[tauri::command]
fn retry_startup(app: tauri::AppHandle, state: State<AppState>) -> StartupHealth {
    let _ = state.stop_preview();
    let resource_dir = app.path().resource_dir().ok();
    let prepared = app_data_path(&app).and_then(|path| {
        prepare_app_data(&path).map(|status| {
            if let Ok(mut migration) = state.migration.lock() {
                *migration = brand_migration::BrandMigrationState::ready(status);
            }
            path
        })
    });
    if let Err(error) = &prepared {
        if let Ok(mut migration) = state.migration.lock() {
            *migration = brand_migration::BrandMigrationState::failed(error.clone());
        }
    }
    state
        .recovery
        .retry_failed_with(prepared.clone(), initialize_core, |data_dir| {
            initialize_packs(data_dir, resource_dir)
        });
    if let Ok(path) = prepared {
        if let Ok(mut jobs) = state.studio_jobs.lock() {
            if jobs.is_err() {
                *jobs = initialize_studio_jobs(&path);
            }
        }
    }
    state.startup_health()
}

fn initialize_core(app_data_dir: &Path) -> Result<Core, String> {
    let database_path = app_data_dir.join("preferences.sqlite3");
    let preferences = PreferencesRepository::open(&database_path).map_err(|error| {
        format!(
            "Local preferences could not open {}: {error}. Check folder permissions or move a damaged database aside, then restart the app.",
            database_path.display()
        )
    })?;
    let reconciled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    SessionAudioCoordinator::restore_at(NativeAudioFacade::new(), preferences, reconciled_at).map_err(|error| {
        format!(
            "Saved preferences could not be restored: {error}. Check the local preferences database, then restart the app."
        )
    })
}

fn initialize_packs(app_data_dir: &Path, resource_dir: Option<PathBuf>) -> Result<Packs, String> {
    let database_path = app_data_dir.join("preferences.sqlite3");
    let registry = PreferencesRepository::open(&database_path).map_err(|error| {
        format!("Installed content registry could not open: {error}. Check local storage permissions and restart the app.")
    })?;
    let mut service =
        PackService::new(registry, app_data_dir.join("content")).with_resource_dir(resource_dir);
    service.list().map_err(|error| {
        format!("Installed content failed its startup integrity check: {error}. Reinstall the affected content pack or restore the local content directory and registry together.")
    })?;
    Ok(service)
}

fn initialize_studio_jobs(app_data_dir: &Path) -> Result<PreferencesRepository, String> {
    PreferencesRepository::open(app_data_dir.join("preferences.sqlite3"))
        .map_err(|_| "Saved Music Studio items are temporarily unavailable.".to_owned())
}

#[tauri::command]
fn get_provenance() -> Result<Provenance, String> {
    let config = audio_engine::tone::ToneConfig::default();
    Ok(Provenance::bundled_test_tone(
        config.sample_rate,
        config.duration_seconds,
    ))
}

#[derive(Debug, serde::Serialize)]
struct CurrentSource {
    pack_id: String,
    pack_title: String,
    item_id: String,
    item_title: String,
    variant_id: String,
    fallback: bool,
    quarantined_review: bool,
    navigation_available: bool,
    /// Bounded `data:` URL for the active installed item's cover art, or
    /// `None` for fallback/review/generated-local sources and items without a
    /// declared cover. The renderer never receives a filesystem path.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_art: Option<String>,
}

impl CurrentSource {
    fn from_audio(
        label: SourceLabel,
        source_kind: PlaybackSourceKind,
        navigation_available: bool,
        cover_art: Option<String>,
    ) -> Self {
        let fallback = source_kind == PlaybackSourceKind::TestTone;
        let quarantined_review = source_kind == PlaybackSourceKind::Review;
        Self {
            pack_id: label.pack_id,
            pack_title: label.pack_title,
            item_id: label.item_id,
            item_title: label.item_title,
            variant_id: label.variant_id,
            fallback,
            quarantined_review,
            navigation_available: navigation_available && !quarantined_review && !fallback,
            cover_art,
        }
    }
}

#[tauri::command]
fn get_current_source(state: State<AppState>) -> Result<CurrentSource, String> {
    // Read the renderer-owned label/kind from the core first, then resolve the
    // cover from the pack service. The two locks are never held simultaneously.
    let (label, source_kind, navigation_available) = {
        let mut guard = state
            .recovery
            .core
            .lock()
            .map_err(|error| error.to_string())?;
        let core = guard.as_mut().map_err(|error| error.clone())?;
        (
            core.source_label(),
            core.source_kind(),
            core.navigation_available(),
        )
    };
    let cover_art = if source_kind == PlaybackSourceKind::Installed {
        state
            .with_packs(|packs| packs.cover_art_data_url(&label.pack_id, &label.item_id))
            .ok()
            .flatten()
    } else {
        None
    };
    Ok(CurrentSource::from_audio(
        label,
        source_kind,
        navigation_available,
        cover_art,
    ))
}

#[tauri::command]
fn get_item_feedback(item_id: String, state: State<AppState>) -> Result<ItemFeedbackState, String> {
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let source = core.source_label();
    packs
        .feedback_state_for_displayed_item(
            core.snapshot(state.now_secs()).activity,
            &item_id,
            &source,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_item_feedback(
    item_id: String,
    focus_feedback: Option<String>,
    enjoyment: Option<String>,
    state: State<AppState>,
) -> Result<ItemFeedbackState, String> {
    let focus_feedback = focus_feedback
        .as_deref()
        .map(parse_track_feedback)
        .transpose()?;
    let enjoyment = enjoyment
        .as_deref()
        .map(parse_track_enjoyment)
        .transpose()?;
    let mut packs_guard = state
        .recovery
        .packs
        .lock()
        .map_err(|error| error.to_string())?;
    let packs = packs_guard.as_mut().map_err(|error| error.clone())?;
    let mut core_guard = state
        .recovery
        .core
        .lock()
        .map_err(|error| error.to_string())?;
    let core = core_guard.as_mut().map_err(|error| error.clone())?;
    let source = core.source_label();
    packs
        .save_feedback_for_displayed_item(
            core.snapshot(state.now_secs()).activity,
            &item_id,
            focus_feedback,
            enjoyment,
            &source,
        )
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let target_path = app_data_path(app.handle());
            let migration_result = target_path
                .as_deref()
                .map_err(Clone::clone)
                .and_then(prepare_app_data);
            let migration = match &migration_result {
                Ok(status) => brand_migration::BrandMigrationState::ready(*status),
                Err(error) => brand_migration::BrandMigrationState::failed(error.clone()),
            };
            let data_dir = migration_result.and(target_path.clone());
            let core = data_dir
                .as_deref()
                .map_err(Clone::clone)
                .and_then(initialize_core);
            let resource_dir = app.path().resource_dir().ok();
            let packs = data_dir
                .as_deref()
                .map_err(Clone::clone)
                .and_then(|data_dir| initialize_packs(data_dir, resource_dir));
            let review = app
                .path()
                .resource_dir()
                .map(ReviewService::new)
                .unwrap_or_else(|_| {
                    ReviewService::new(PathBuf::from("__missing_review_resources__"))
                });
            let studio_jobs = data_dir
                .as_deref()
                .map_err(Clone::clone)
                .and_then(initialize_studio_jobs);
            let package_source = app
                .path()
                .resource_dir()
                .map(|p| p.join("music-studio-runtime"))
                .unwrap_or_else(|_| PathBuf::from("__missing_music_studio_package__"));
            let studio_paths = data_dir
                .as_deref()
                .map(|p| music_studio::StudioRuntimePaths::for_app_data(p, package_source.clone()))
                .unwrap_or_else(|_| {
                    music_studio::StudioRuntimePaths::for_app_data(
                        Path::new("__missing_app_data__"),
                        package_source,
                    )
                });
            let studio_installer = music_studio::RuntimeInstaller::new(studio_paths.clone());
            let studio_generation_service =
                studio_generation::GenerationService::production(studio_paths.clone());
            let _ = studio_generation_service.recover();
            app.manage(AppState {
                recovery: RecoverySlots {
                    core: Mutex::new(core),
                    packs: Mutex::new(packs),
                },
                review,
                studio_paths,
                studio_installer,
                studio_jobs: Mutex::new(studio_jobs),
                studio_generation: Arc::new(Mutex::new(())),
                studio_active: Arc::new(AtomicBool::new(false)),
                studio_generation_service,
                preview: Mutex::new(PreviewAudioCoordinator::new(NativeAudioFacade::new())),
                clock_base: std::time::Instant::now(),
                migration: Mutex::new(migration),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            list_favorites,
            remove_favorite,
            start_favorite,
            list_my_music,
            rename_my_music,
            delete_my_music,
            start_my_music,
            get_onboarding_preferences,
            complete_onboarding,
            list_review_candidates,
            start_review_candidate,
            pause_session,
            resume_session,
            stop_session,
            next_track,
            previous_track,
            set_activity,
            get_activity_genres,
            set_activity_genre,
            get_activity_moods,
            set_activity_mood,
            set_intensity,
            get_master_volume,
            set_master_volume,
            set_session_type,
            get_snapshot,
            list_recent_sessions,
            save_session_rating,
            get_provenance,
            get_current_source,
            get_item_feedback,
            set_item_feedback,
            list_content_packs,
            import_content_pack,
            get_startup_health,
            retry_startup,
            get_studio_capability,
            start_runtime_install,
            get_runtime_install,
            cancel_runtime_install,
            repair_runtime,
            list_recent_studio_jobs,
            get_studio_job,
            create_studio_music,
            cancel_studio_music,
            start_draft_preview,
            pause_draft_preview,
            resume_draft_preview,
            stop_draft_preview,
            get_draft_preview_state,
            save_studio_draft,
            discard_studio_draft,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. }) {
                if let Some(state) = app.try_state::<AppState>() {
                    state.studio_generation_service.shutdown();
                    let _ = state.stop_preview();
                }
            }
        });
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn studio_job_dtos_use_customer_statuses_and_fixed_recent_cap() {
        assert_eq!(RECENT_STUDIO_JOB_LIMIT, 12);
        assert_eq!(
            studio_job_status(music_studio_domain::StudioJobState::Ready),
            "Ready"
        );
        assert_eq!(
            studio_job_status(music_studio_domain::StudioJobState::Saved),
            "Saved"
        );
        assert_eq!(
            studio_job_status(music_studio_domain::StudioJobState::Generating),
            "In progress"
        );
        assert_eq!(
            studio_job_status(music_studio_domain::StudioJobState::Failed),
            "Needs attention"
        );
    }

    #[test]
    fn only_playing_and_paused_sessions_hold_live_audio() {
        assert!(session_holds_live_audio(domain::SessionStatus::Playing));
        assert!(session_holds_live_audio(domain::SessionStatus::Paused));
        assert!(!session_holds_live_audio(domain::SessionStatus::Idle));
        assert!(!session_holds_live_audio(domain::SessionStatus::Stopped));
        assert!(!session_holds_live_audio(domain::SessionStatus::Expired));
    }

    #[test]
    fn studio_job_read_failures_map_to_a_safe_message() {
        let error = map_recent_studio_jobs(Err(persistence::PersistenceError::InvalidStudioJob))
            .unwrap_err();
        assert_eq!(
            error,
            "Saved Music Studio items could not be read right now."
        );
    }

    #[test]
    fn ready_draft_builds_a_valid_app_owned_generated_record() {
        let temp = tempfile::tempdir().unwrap();
        let paths = music_studio::StudioRuntimePaths::for_app_data(
            temp.path(),
            temp.path().join("unused-package"),
        );
        let request = music_studio_domain::StudioPromptInput::new(
            Activity::DeepWork,
            music_studio_domain::StudioId::new("ambient").unwrap(),
            None,
            music_studio_domain::StudioEnergy::Medium,
            vec![],
            None,
            None,
            music_studio_domain::StudioDuration::Seconds180,
        )
        .unwrap();
        let job = music_studio_domain::StudioJobRecord {
            job_id: music_studio_domain::StudioJobId::new("job_0123456789abcdef").unwrap(),
            attempt_id: music_studio_domain::StudioAttemptId::new("attempt_0123456789abcdef")
                .unwrap(),
            prompt: music_studio_domain::build_studio_prompt(&request, 42).unwrap(),
            request,
            state: music_studio_domain::StudioJobState::Ready,
            revision: 3,
            created_at_ms: 10,
            updated_at_ms: 20,
            failure: None,
        };
        let draft = paths.draft_output_path(&job.job_id);
        std::fs::create_dir_all(draft.parent().unwrap()).unwrap();
        std::fs::write(&draft, b"test-draft").unwrap();
        let artifact = StudioJobArtifact {
            job_id: job.job_id.clone(),
            parent_job_id: None,
            runtime_version: "test-v1".into(),
            stage: "ready".into(),
            output_relative_path: Some(format!("drafts/{}.flac", job.job_id.as_str())),
            output_sha256: Some(sha256_file(&draft).unwrap()),
            analysis_json: Some(
                r#"{"codec":"flac","sample_rate_hz":48000,"channels":2,"duration_seconds":180.0,"vocal_speech":"not_assessed"}"#.into(),
            ),
            safe_error_code: None,
            created_at_ms: 10,
            updated_at_ms: 20,
        };

        let (record, source) =
            ready_generated_record(&job, &artifact, &paths, "Test focus".into()).unwrap();
        assert_eq!(source, draft);
        assert_eq!(record.evidence.producer, "adhd-music-studio");
        record.validate().unwrap();
    }

    #[test]
    fn recovered_save_is_idempotent_only_for_the_same_customer_title() {
        let item = pack_service::MyMusicItem {
            item_id: "generated.local.job_0123456789abcdef.item".into(),
            title: "Test focus".into(),
            duration_seconds: 180,
            created_at: 42,
            activity: Activity::DeepWork,
            job_id: "job_0123456789abcdef".into(),
        };

        assert_eq!(
            reconciled_saved_item(vec![item.clone()], "job_0123456789abcdef", "Test focus")
                .unwrap(),
            Some(item.clone())
        );
        assert_eq!(
            reconciled_saved_item(vec![item], "job_0123456789abcdef", "Another name").unwrap_err(),
            "This music was already saved with a different name."
        );
        assert!(
            reconciled_saved_item(Vec::new(), "job_0123456789abcdef", "Test focus")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn feedback_command_boundary_accepts_only_explicit_independent_values() {
        assert_eq!(
            parse_track_feedback("helps_focus").unwrap(),
            TrackFeedback::HelpsFocus
        );
        assert_eq!(
            parse_track_feedback("neutral").unwrap(),
            TrackFeedback::Neutral
        );
        assert_eq!(
            parse_track_enjoyment("liked").unwrap(),
            TrackEnjoyment::Liked
        );
        assert!(parse_track_enjoyment("distracting").is_err());
    }

    #[test]
    fn draft_preview_rejects_active_focus_transport_with_a_friendly_message() {
        assert!(ensure_focus_is_idle_for_draft_preview(domain::SessionStatus::Idle).is_ok());
        assert_eq!(
            ensure_focus_is_idle_for_draft_preview(domain::SessionStatus::Playing),
            Err("Stop focus playback before previewing a Music Studio draft.".to_owned())
        );
        assert!(ensure_focus_is_idle_for_draft_preview(domain::SessionStatus::Paused).is_err());
    }

    #[test]
    fn review_current_source_serialization_stays_blind() {
        let source = CurrentSource::from_audio(
            SourceLabel {
                pack_id: "quarantined-activity-review".into(),
                pack_title: "Quarantined Activity Review — not approved/published".into(),
                item_id: "blind-m".into(),
                item_title: "Track M — quarantined review".into(),
                variant_id: "opaque-review-source".into(),
            },
            PlaybackSourceKind::Review,
            false,
            None,
        );
        let serialized = serde_json::to_string(&source).unwrap();

        assert!(source.quarantined_review);
        assert!(source.cover_art.is_none());
        assert!(serialized.contains("blind-m"));
        assert!(!serialized.contains("cover_art"));
        for forbidden in [
            "creativity-softmotion-downtempo-086.flac",
            "75fdcc6b23b967fcf82a10f45ea423af884a9da0b5c0529fdfcff40ea2ce63c2",
            "creativity",
            "motivation",
            "lightwork",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn current_source_serializes_cover_art_for_installed_packs() {
        let source = CurrentSource::from_audio(
            SourceLabel {
                pack_id: "test.focus-pack".into(),
                pack_title: "Test Focus Pack".into(),
                item_id: "test-item".into(),
                item_title: "Generated byte fixture".into(),
                variant_id: "source".into(),
            },
            PlaybackSourceKind::Installed,
            true,
            Some("data:image/png;base64,aGVsbG8=".to_owned()),
        );
        let serialized = serde_json::to_string(&source).unwrap();
        assert!(serialized.contains("\"cover_art\":\"data:image/png;base64,aGVsbG8=\""));
        assert!(!source.fallback);
        assert!(!source.quarantined_review);
        assert_eq!(
            source.cover_art.as_deref(),
            Some("data:image/png;base64,aGVsbG8=")
        );
    }

    fn slots(core: Result<u32, &str>, packs: Result<u32, &str>) -> RecoverySlots<u32, u32> {
        RecoverySlots {
            core: Mutex::new(core.map_err(str::to_owned)),
            packs: Mutex::new(packs.map_err(str::to_owned)),
        }
    }

    #[test]
    fn failed_services_recover_without_hardware() {
        let slots = slots(Err("audio unavailable"), Err("content unavailable"));
        let health =
            slots.retry_failed_with(Ok(PathBuf::from("safe-data")), |_| Ok(10), |_| Ok(20));
        assert_eq!(
            health,
            StartupHealth {
                core_ready: true,
                core_error: None,
                packs_ready: true,
                packs_error: None,
                migration_status: brand_migration::BrandMigrationStatus::NotNeeded,
                migration_error: None
            }
        );
    }

    #[test]
    fn healthy_service_identity_is_preserved_and_partial_failure_is_reported() {
        let slots = slots(Ok(7), Err("content unavailable"));
        let health = slots.retry_failed_with(
            Ok(PathBuf::from("safe-data")),
            |_| Ok(99),
            |_| Err("still locked".to_owned()),
        );
        assert_eq!(*slots.core.lock().unwrap().as_ref().unwrap(), 7);
        assert!(health.core_ready);
        assert_eq!(health.packs_error.as_deref(), Some("still locked"));
    }

    #[test]
    fn repeated_retry_is_idempotent_and_does_not_construct_healthy_services() {
        let slots = slots(Err("audio unavailable"), Err("content unavailable"));
        let _ = slots.retry_failed_with(Ok(PathBuf::from("safe-data")), |_| Ok(1), |_| Ok(2));
        let health = slots.retry_failed_with(
            Ok(PathBuf::from("safe-data")),
            |_| panic!("healthy core must not be rebuilt"),
            |_| panic!("healthy packs must not be rebuilt"),
        );
        assert!(health.core_ready && health.packs_ready);
    }

    #[test]
    fn compare_before_commit_never_overwrites_a_recovered_service() {
        let slots = slots(Err("audio unavailable"), Err("content unavailable"));
        slots.commit_core_if_failed(Ok(1));
        slots.commit_core_if_failed(Ok(2));
        assert_eq!(*slots.core.lock().unwrap().as_ref().unwrap(), 1);
    }

    #[test]
    fn concurrent_retries_leave_each_slot_recovered_once() {
        let slots =
            std::sync::Arc::new(slots(Err("audio unavailable"), Err("content unavailable")));
        let first = std::sync::Arc::clone(&slots);
        let second = std::sync::Arc::clone(&slots);
        let a = std::thread::spawn(move || {
            first.retry_failed_with(Ok(PathBuf::from("safe-data")), |_| Ok(1), |_| Ok(1))
        });
        let b = std::thread::spawn(move || {
            second.retry_failed_with(Ok(PathBuf::from("safe-data")), |_| Ok(2), |_| Ok(2))
        });
        assert!(a.join().unwrap().core_ready);
        assert!(b.join().unwrap().packs_ready);
        assert!(slots.health().core_ready && slots.health().packs_ready);
    }

    #[test]
    fn path_failure_updates_only_failed_slots_without_destructive_repair() {
        let slots = slots(Ok(7), Err("content unavailable"));
        let health = slots.retry_failed_with(
            Err("app data path unavailable".to_owned()),
            |_| panic!("no constructor on path failure"),
            |_| panic!("no constructor on path failure"),
        );
        assert_eq!(*slots.core.lock().unwrap().as_ref().unwrap(), 7);
        assert_eq!(
            health.packs_error.as_deref(),
            Some("app data path unavailable")
        );
    }
}
