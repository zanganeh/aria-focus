use crate::music_studio::StudioRuntimePaths;
use music_studio_domain::{
    build_studio_prompt, StudioAttemptId, StudioDuration, StudioEnergy, StudioErrorCode,
    StudioFailureDetails, StudioId, StudioJobId, StudioJobRecord, StudioJobState,
    StudioPromptInput,
};
use persistence::{PreferencesRepository, StudioJobArtifact, StudioJobStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ACTIVE_MESSAGE: &str =
    "Music is already being created. You can make another one when it is finished.";
const STORE_MESSAGE: &str = "Music Studio is temporarily unavailable.";
const WORKER_TIMEOUT: Duration = Duration::from_secs(65 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_WORKER_RESULT_BYTES: u64 = 4096;
const DURATION_TOLERANCE_SECONDS: f64 = 1.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateStudioMusicRequest {
    pub activity: domain::Activity,
    pub sound_style_id: String,
    pub energy: StudioEnergy,
    pub duration_seconds: StudioDuration,
    pub note: Option<String>,
    pub parent_job_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerSpec {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub request_path: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerOutcome {
    Success,
    Cancelled,
    Timeout,
    GpuOutOfMemory,
    UnexpectedExit,
    InvalidOutput,
}

pub trait Worker: Send + Sync {
    fn run(&self, spec: &WorkerSpec, cancel: &AtomicBool) -> WorkerOutcome;
}

#[derive(Default)]
pub struct ProcessWorker;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerResult {
    ok: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    output: Option<String>,
}

impl Worker for ProcessWorker {
    fn run(&self, spec: &WorkerSpec, cancel: &AtomicBool) -> WorkerOutcome {
        let mut command = Command::new(&spec.python);
        command
            .arg(&spec.worker)
            .arg("--request")
            .arg(&spec.request_path)
            .arg("--output-dir")
            .arg(&spec.output_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut child) = command.spawn() else {
            return WorkerOutcome::UnexpectedExit;
        };
        #[cfg(windows)]
        let job = match WindowsJob::assign(&child) {
            Ok(job) => Some(job),
            Err(()) => {
                let _ = child.kill();
                let _ = child.wait();
                return WorkerOutcome::UnexpectedExit;
            }
        };
        let started = Instant::now();
        let status = loop {
            if cancel.load(Ordering::SeqCst) {
                #[cfg(windows)]
                if let Some(job) = &job {
                    job.terminate();
                }
                let _ = child.kill();
                let _ = child.wait();
                return WorkerOutcome::Cancelled;
            }
            if started.elapsed() >= WORKER_TIMEOUT {
                #[cfg(windows)]
                if let Some(job) = &job {
                    job.terminate();
                }
                let _ = child.kill();
                let _ = child.wait();
                return WorkerOutcome::Timeout;
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(_) => {
                    #[cfg(windows)]
                    if let Some(job) = &job {
                        job.terminate();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    return WorkerOutcome::UnexpectedExit;
                }
            }
        };
        let mut bytes = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            if stdout
                .take(MAX_WORKER_RESULT_BYTES + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() as u64 > MAX_WORKER_RESULT_BYTES
            {
                return WorkerOutcome::UnexpectedExit;
            }
        }
        let result = serde_json::from_slice::<WorkerResult>(&bytes).ok();
        if status.success()
            && matches!(result, Some(WorkerResult { ok: true, output: Some(ref output), .. }) if output == "draft.flac")
        {
            return WorkerOutcome::Success;
        }
        match result.and_then(|value| value.code).as_deref() {
            Some("timeout") => WorkerOutcome::Timeout,
            Some("gpu_oom") => WorkerOutcome::GpuOutOfMemory,
            Some("invalid_output") => WorkerOutcome::InvalidOutput,
            _ => WorkerOutcome::UnexpectedExit,
        }
    }
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsJob {
    fn assign(child: &std::process::Child) -> Result<Self, ()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(());
            }
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &information as *const _ as *const _,
                std::mem::size_of_val(&information) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle() as _) == 0
            {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                return Err(());
            }
            Ok(Self(handle))
        }
    }
    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisSummary {
    schema_version: u8,
    codec: &'static str,
    sample_rate_hz: u32,
    channels: u16,
    duration_seconds: f64,
    clipped_samples: u64,
    non_finite_samples: u64,
    vocal_speech: &'static str,
}

pub trait DraftAnalyzer: Send + Sync {
    fn analyze(&self, path: &Path, requested_duration: u16) -> Result<AnalysisSummary, ()>;
}

#[derive(Default)]
pub struct ProductionAnalyzer;

impl DraftAnalyzer for ProductionAnalyzer {
    fn analyze(&self, path: &Path, requested_duration: u16) -> Result<AnalysisSummary, ()> {
        let report = audio_analyzer::analyze_file(path);
        let duration = report.decode.duration_seconds.ok_or(())?;
        if report.decode.status != audio_analyzer::DecodeStatus::Decoded
            || report.decode.codec != Some(audio_analyzer::Codec::Flac)
            || report.decode.sample_rate_hz != Some(48_000)
            || report.decode.channels != Some(2)
            || (duration - f64::from(requested_duration)).abs() > DURATION_TOLERANCE_SECONDS
            || report.measurements.non_finite_samples != 0
            || report.measurements.clipped_samples != 0
            || report.has_hard_rejections()
        {
            return Err(());
        }
        Ok(AnalysisSummary {
            schema_version: 1,
            codec: "flac",
            sample_rate_hz: 48_000,
            channels: 2,
            duration_seconds: duration,
            clipped_samples: 0,
            non_finite_samples: 0,
            vocal_speech: "not_assessed",
        })
    }
}

struct ActiveGeneration {
    job_id: StudioJobId,
    cancel: Arc<AtomicBool>,
    finished: Arc<(Mutex<bool>, Condvar)>,
}

struct Reservation<'a> {
    active: &'a Mutex<Option<ActiveGeneration>>,
    armed: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(mut active) = self.active.lock() {
                *active = None;
            }
        }
    }
}

pub struct GenerationService {
    paths: StudioRuntimePaths,
    worker: Arc<dyn Worker>,
    analyzer: Arc<dyn DraftAnalyzer>,
    active: Mutex<Option<ActiveGeneration>>,
    #[cfg(test)]
    runtime_override: Option<(PathBuf, String)>,
}

impl GenerationService {
    pub fn production(paths: StudioRuntimePaths) -> Arc<Self> {
        Arc::new(Self {
            paths,
            worker: Arc::new(ProcessWorker),
            analyzer: Arc::new(ProductionAnalyzer),
            active: Mutex::new(None),
            #[cfg(test)]
            runtime_override: None,
        })
    }

    #[cfg(test)]
    fn injected(
        paths: StudioRuntimePaths,
        worker: Arc<dyn Worker>,
        analyzer: Arc<dyn DraftAnalyzer>,
        runtime_root: PathBuf,
        runtime_version: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            paths,
            worker,
            analyzer,
            active: Mutex::new(None),
            runtime_override: Some((runtime_root, runtime_version)),
        })
    }

    pub fn create(
        self: &Arc<Self>,
        request: CreateStudioMusicRequest,
    ) -> Result<StudioJobRecord, String> {
        let style = match request.sound_style_id.as_str() {
            "ambient" | "gentle-piano" | "soft-electronic" => request.sound_style_id,
            _ => return Err("Please choose a sound style.".into()),
        };
        if request
            .note
            .as_ref()
            .is_some_and(|note| note.chars().count() > 240)
        {
            return Err("Please check your music choices and try again.".into());
        }
        let input = StudioPromptInput::new(
            request.activity,
            StudioId::new(style).map_err(|_| "Please choose a sound style.".to_owned())?,
            None,
            request.energy,
            Vec::new(),
            request.note,
            None,
            request.duration_seconds,
        )
        .map_err(|_| "Please check your music choices and try again.".to_owned())?;
        let parent = request
            .parent_job_id
            .map(StudioJobId::new)
            .transpose()
            .map_err(|_| "That music could not be found.".to_owned())?;
        let job_id = random_id("job")?;
        let attempt_id = random_attempt_id()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let finished = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let mut active = self.active.lock().map_err(|_| STORE_MESSAGE.to_owned())?;
            if active.is_some() {
                return Err(ACTIVE_MESSAGE.into());
            }
            *active = Some(ActiveGeneration {
                job_id: job_id.clone(),
                cancel: cancel.clone(),
                finished: finished.clone(),
            });
        }
        let mut reservation = Reservation {
            active: &self.active,
            armed: true,
        };
        let mut store = PreferencesRepository::open(&self.paths.database_path)
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        if store
            .recent_studio_jobs(persistence::MAX_RECENT_STUDIO_JOBS)
            .map_err(|_| STORE_MESSAGE.to_owned())?
            .iter()
            .any(|job| {
                matches!(
                    job.state,
                    StudioJobState::Queued | StudioJobState::Generating | StudioJobState::Analyzing
                )
            })
        {
            return Err(ACTIVE_MESSAGE.into());
        }
        if let Some(parent_id) = &parent {
            if store
                .load_studio_job(parent_id)
                .map_err(|_| STORE_MESSAGE.to_owned())?
                .is_none()
            {
                return Err("That music could not be found.".into());
            }
        }
        let now = now_ms();
        let seed = random_u64()?;
        let prompt = build_studio_prompt(&input, seed)
            .map_err(|_| "Please check your music choices and try again.".to_owned())?;
        let record = StudioJobRecord::new(job_id, attempt_id, input, prompt, now)
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        store
            .create_studio_job(&record)
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        drop(store);
        let service = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name(format!("studio-{}", record.job_id.as_str()))
            .spawn({
                let record = record.clone();
                move || service.run(record, parent, cancel, finished)
            });
        if spawned.is_err() {
            let mut store = PreferencesRepository::open(&self.paths.database_path)
                .map_err(|_| STORE_MESSAGE.to_owned())?;
            let _ = fail_job(&mut store, &record, "spawn_failed");
            return Err("Music could not be started. Please try again.".into());
        }
        reservation.armed = false;
        Ok(record)
    }

    fn run(
        &self,
        queued: StudioJobRecord,
        parent_job_id: Option<StudioJobId>,
        cancel: Arc<AtomicBool>,
        finished: Arc<(Mutex<bool>, Condvar)>,
    ) {
        self.run_inner(queued, parent_job_id, &cancel);
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
        let (done, wake) = &*finished;
        if let Ok(mut done) = done.lock() {
            *done = true;
            wake.notify_all();
        }
    }

    fn run_inner(
        &self,
        queued: StudioJobRecord,
        parent_job_id: Option<StudioJobId>,
        cancel: &AtomicBool,
    ) {
        let Ok(mut store) = PreferencesRepository::open(&self.paths.database_path) else {
            return;
        };
        if cancel.load(Ordering::SeqCst) {
            let _ = cancel_job(&mut store, &queued);
            return;
        }
        #[cfg(test)]
        let runtime = self
            .runtime_override
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.paths.verified_installed_runtime());
        #[cfg(not(test))]
        let runtime = self.paths.verified_installed_runtime();
        let Ok((runtime_root, runtime_version)) = runtime else {
            let _ = fail_job(&mut store, &queued, "runtime_invalid");
            return;
        };
        let Ok(generating) = store.transition_studio_job(
            &queued.job_id,
            queued.revision,
            StudioJobState::Generating,
            now_ms(),
            None,
        ) else {
            return;
        };
        let output_dir = self.paths.job_output_dir(&queued.job_id);
        let request_path = self.paths.job_request_path(&queued.job_id);
        if controlled_remove(&output_dir).is_err()
            || controlled_remove(&request_path).is_err()
            || fs::create_dir_all(output_dir.parent().unwrap_or(&self.paths.resources_dir)).is_err()
        {
            let _ = fail_job(&mut store, &generating, "output_invalid");
            return;
        }
        let worker_request = serde_json::json!({
            "positive_prompt": queued.prompt.creative_prompt,
            "negative_prompt": queued.prompt.locked_negative_prompt,
            "duration_seconds": queued.prompt.duration_seconds,
            "seed": queued.prompt.seed,
        });
        if fs::write(
            &request_path,
            serde_json::to_vec(&worker_request).unwrap_or_default(),
        )
        .is_err()
        {
            let _ = fail_job(&mut store, &generating, "output_invalid");
            return;
        }
        let python = if cfg!(windows) {
            runtime_root.join(".venv/Scripts/python.exe")
        } else {
            runtime_root.join(".venv/bin/python")
        };
        let outcome = self.worker.run(
            &WorkerSpec {
                python,
                worker: runtime_root.join("studio_worker.py"),
                request_path: request_path.clone(),
                output_dir: output_dir.clone(),
            },
            cancel,
        );
        let _ = controlled_remove(&request_path);
        if outcome == WorkerOutcome::Cancelled || cancel.load(Ordering::SeqCst) {
            let _ = controlled_remove(&output_dir);
            let _ = cancel_job(&mut store, &generating);
            return;
        }
        if outcome != WorkerOutcome::Success {
            let _ = controlled_remove(&output_dir);
            let code = match outcome {
                WorkerOutcome::Timeout => "timeout",
                WorkerOutcome::GpuOutOfMemory => "gpu_oom",
                WorkerOutcome::InvalidOutput => "output_invalid",
                _ => "unexpected_exit",
            };
            let _ = fail_job(&mut store, &generating, code);
            return;
        }
        let Ok(draft) = exact_draft(&output_dir) else {
            let _ = controlled_remove(&output_dir);
            let _ = fail_job(&mut store, &generating, "output_invalid");
            return;
        };
        let Ok(hash_before) = sha256_regular(&draft) else {
            let _ = controlled_remove(&output_dir);
            let _ = fail_job(&mut store, &generating, "output_invalid");
            return;
        };
        let Ok(analyzing) = store.transition_studio_job(
            &queued.job_id,
            generating.revision,
            StudioJobState::Analyzing,
            now_ms(),
            None,
        ) else {
            return;
        };
        let analysis = self
            .analyzer
            .analyze(&draft, queued.prompt.duration_seconds);
        let hash_after = sha256_regular(&draft);
        if cancel.load(Ordering::SeqCst) {
            let _ = controlled_remove(&output_dir);
            let _ = cancel_job(&mut store, &analyzing);
            return;
        }
        if analysis.is_err() || hash_after.as_ref() != Ok(&hash_before) {
            let _ = controlled_remove(&output_dir);
            let _ = store.transition_studio_job(
                &queued.job_id,
                analyzing.revision,
                StudioJobState::Rejected,
                now_ms(),
                None,
            );
            return;
        }
        let analysis_json = serde_json::to_string(&analysis.unwrap()).ok();
        let target = self.paths.draft_output_path(&queued.job_id);
        if fs::create_dir_all(target.parent().unwrap_or(&self.paths.resources_dir)).is_err()
            || target.exists()
            || fs::rename(&draft, &target).is_err()
        {
            let _ = controlled_remove(&output_dir);
            let _ = fail_job(&mut store, &analyzing, "output_invalid");
            return;
        }
        let artifact = StudioJobArtifact {
            job_id: queued.job_id.clone(),
            parent_job_id,
            runtime_version,
            stage: "ready".into(),
            output_relative_path: Some(format!("drafts/{}.flac", queued.job_id.as_str())),
            output_sha256: Some(hash_before),
            analysis_json,
            safe_error_code: None,
            created_at_ms: queued.created_at_ms,
            updated_at_ms: now_ms(),
        };
        if store.upsert_studio_job_artifact(&artifact).is_err()
            || store
                .transition_studio_job(
                    &queued.job_id,
                    analyzing.revision,
                    StudioJobState::Ready,
                    now_ms(),
                    None,
                )
                .is_err()
        {
            let _ = fs::remove_file(&target);
        }
        let _ = controlled_remove(&output_dir);
    }

    pub fn cancel(&self, job_id: &StudioJobId) -> Result<StudioJobRecord, String> {
        let finished = {
            let active = self.active.lock().map_err(|_| STORE_MESSAGE.to_owned())?;
            let active = active
                .as_ref()
                .ok_or_else(|| "That music is no longer being created.".to_owned())?;
            if &active.job_id != job_id {
                return Err("That music is no longer being created.".into());
            }
            active.cancel.store(true, Ordering::SeqCst);
            active.finished.clone()
        };
        let (done, wake) = &*finished;
        let mut done = done.lock().map_err(|_| STORE_MESSAGE.to_owned())?;
        while !*done {
            done = wake.wait(done).map_err(|_| STORE_MESSAGE.to_owned())?;
        }
        let mut store = PreferencesRepository::open(&self.paths.database_path)
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        store
            .load_studio_job(job_id)
            .map_err(|_| STORE_MESSAGE.to_owned())?
            .ok_or_else(|| "That music could not be found.".into())
    }

    pub fn shutdown(&self) {
        let active = self.active.lock().ok().and_then(|active| {
            active
                .as_ref()
                .map(|active| (active.cancel.clone(), active.finished.clone()))
        });
        if let Some((cancel, finished)) = active {
            cancel.store(true, Ordering::SeqCst);
            let (done, wake) = &*finished;
            if let Ok(done) = done.lock() {
                let _ = wake.wait_timeout_while(done, Duration::from_secs(10), |done| !*done);
            }
        }
    }

    pub fn recover(&self) -> Result<(), String> {
        let mut store = PreferencesRepository::open(&self.paths.database_path)
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        let running: Vec<_> = store
            .recent_studio_jobs(persistence::MAX_RECENT_STUDIO_JOBS)
            .map_err(|_| STORE_MESSAGE.to_owned())?
            .into_iter()
            .filter(|job| {
                matches!(
                    job.state,
                    StudioJobState::Queued
                        | StudioJobState::Generating
                        | StudioJobState::Analyzing
                        | StudioJobState::Saving
                )
            })
            .map(|job| job.job_id)
            .collect();
        store
            .recover_studio_jobs(now_ms())
            .map_err(|_| STORE_MESSAGE.to_owned())?;
        for job_id in running {
            let _ = controlled_remove(&self.paths.job_output_dir(&job_id));
            let _ = controlled_remove(&self.paths.job_request_path(&job_id));
        }
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn random_u64() -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| STORE_MESSAGE.to_owned())?;
    Ok(u64::from_le_bytes(bytes))
}

fn random_id(prefix: &str) -> Result<StudioJobId, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| STORE_MESSAGE.to_owned())?;
    StudioJobId::new(format!("{prefix}_{}", hex(&bytes))).map_err(|_| STORE_MESSAGE.to_owned())
}

fn random_attempt_id() -> Result<StudioAttemptId, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| STORE_MESSAGE.to_owned())?;
    StudioAttemptId::new(format!("attempt_{}", hex(&bytes))).map_err(|_| STORE_MESSAGE.to_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn failure(code: &str) -> Option<StudioFailureDetails> {
    StudioFailureDetails::new(StudioErrorCode::InvalidRequest, code.to_owned()).ok()
}

fn fail_job(
    store: &mut dyn StudioJobStore,
    job: &StudioJobRecord,
    code: &str,
) -> Result<StudioJobRecord, ()> {
    store
        .transition_studio_job(
            &job.job_id,
            job.revision,
            StudioJobState::Failed,
            now_ms(),
            failure(code),
        )
        .map_err(|_| ())
}

fn cancel_job(
    store: &mut dyn StudioJobStore,
    job: &StudioJobRecord,
) -> Result<StudioJobRecord, ()> {
    store
        .transition_studio_job(
            &job.job_id,
            job.revision,
            StudioJobState::Cancelled,
            now_ms(),
            None,
        )
        .map_err(|_| ())
}

fn controlled_remove(path: &Path) -> Result<(), ()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            fs::remove_file(path).map_err(|_| ())
        }
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).map_err(|_| ()),
        Ok(_) => Err(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
    }
}

fn exact_draft(output_dir: &Path) -> Result<PathBuf, ()> {
    let metadata = fs::symlink_metadata(output_dir).map_err(|_| ())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(());
    }
    let mut flacs = Vec::new();
    for entry in fs::read_dir(output_dir).map_err(|_| ())? {
        let path = entry.map_err(|_| ())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("flac"))
        {
            if !metadata.is_file() {
                return Err(());
            }
            flacs.push(path);
        }
    }
    if flacs.len() == 1
        && flacs[0].file_name().and_then(|value| value.to_str()) == Some("draft.flac")
    {
        Ok(flacs.remove(0))
    } else {
        Err(())
    }
}

fn sha256_regular(path: &Path) -> Result<String, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(());
    }
    let mut stream = fs::File::open(path).map_err(|_| ())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = stream.read(&mut buffer).map_err(|_| ())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone, Copy)]
    enum OutputMode {
        Draft,
        Missing,
        Multiple,
    }

    struct Gate {
        started: (Mutex<bool>, Condvar),
        release: (Mutex<bool>, Condvar),
        exited: AtomicBool,
    }

    impl Gate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                started: (Mutex::new(false), Condvar::new()),
                release: (Mutex::new(false), Condvar::new()),
                exited: AtomicBool::new(false),
            })
        }

        fn wait_started(&self) {
            let (started, wake) = &self.started;
            let started = started.lock().unwrap();
            let (started, _) = wake
                .wait_timeout_while(started, Duration::from_secs(2), |value| !*value)
                .unwrap();
            assert!(*started, "fake worker did not start");
        }

        fn release(&self) {
            let (released, wake) = &self.release;
            *released.lock().unwrap() = true;
            wake.notify_all();
        }
    }

    struct FakeWorker {
        database: PathBuf,
        outcome: WorkerOutcome,
        output: OutputMode,
        gate: Option<Arc<Gate>>,
        states: Arc<Mutex<Vec<StudioJobState>>>,
    }

    impl Worker for FakeWorker {
        fn run(&self, spec: &WorkerSpec, cancel: &AtomicBool) -> WorkerOutcome {
            let job_id = StudioJobId::new(
                spec.output_dir
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            )
            .unwrap();
            let mut store = PreferencesRepository::open(&self.database).unwrap();
            self.states
                .lock()
                .unwrap()
                .push(store.load_studio_job(&job_id).unwrap().unwrap().state);
            drop(store);
            if let Some(gate) = &self.gate {
                let (started, wake) = &gate.started;
                *started.lock().unwrap() = true;
                wake.notify_all();
                let (released, wake) = &gate.release;
                let mut released = released.lock().unwrap();
                while !*released && !cancel.load(Ordering::SeqCst) {
                    released = wake
                        .wait_timeout(released, Duration::from_millis(5))
                        .unwrap()
                        .0;
                }
                if cancel.load(Ordering::SeqCst) {
                    gate.exited.store(true, Ordering::SeqCst);
                    return WorkerOutcome::Cancelled;
                }
            }
            if self.outcome == WorkerOutcome::Success {
                fs::create_dir(&spec.output_dir).unwrap();
                match self.output {
                    OutputMode::Draft => {
                        fs::write(spec.output_dir.join("draft.flac"), b"draft-a").unwrap()
                    }
                    OutputMode::Missing => {}
                    OutputMode::Multiple => {
                        fs::write(spec.output_dir.join("draft.flac"), b"draft-a").unwrap();
                        fs::write(spec.output_dir.join("other.flac"), b"draft-b").unwrap();
                    }
                }
            }
            if let Some(gate) = &self.gate {
                gate.exited.store(true, Ordering::SeqCst);
            }
            self.outcome
        }
    }

    struct FakeAnalyzer {
        database: PathBuf,
        states: Arc<Mutex<Vec<StudioJobState>>>,
        reject: bool,
        mutate: bool,
        calls: AtomicUsize,
    }

    impl DraftAnalyzer for FakeAnalyzer {
        fn analyze(&self, path: &Path, requested_duration: u16) -> Result<AnalysisSummary, ()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let job_name = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            let id = StudioJobId::new(job_name.to_string()).unwrap();
            let mut store = PreferencesRepository::open(&self.database).unwrap();
            self.states
                .lock()
                .unwrap()
                .push(store.load_studio_job(&id).unwrap().unwrap().state);
            if self.mutate {
                fs::write(path, b"substituted-after-analysis-start").unwrap();
            }
            if self.reject {
                return Err(());
            }
            Ok(AnalysisSummary {
                schema_version: 1,
                codec: "flac",
                sample_rate_hz: 48_000,
                channels: 2,
                duration_seconds: f64::from(requested_duration),
                clipped_samples: 0,
                non_finite_samples: 0,
                vocal_speech: "not_assessed",
            })
        }
    }

    struct Harness {
        _temp: tempfile::TempDir,
        paths: StudioRuntimePaths,
    }

    impl Harness {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let paths =
                StudioRuntimePaths::for_app_data(temp.path(), temp.path().join("unused-package"));
            PreferencesRepository::open(&paths.database_path).unwrap();
            Self { _temp: temp, paths }
        }

        fn service(
            &self,
            outcome: WorkerOutcome,
            output: OutputMode,
            gate: Option<Arc<Gate>>,
            reject: bool,
            mutate: bool,
        ) -> (Arc<GenerationService>, Arc<Mutex<Vec<StudioJobState>>>) {
            let states = Arc::new(Mutex::new(Vec::new()));
            let worker = Arc::new(FakeWorker {
                database: self.paths.database_path.clone(),
                outcome,
                output,
                gate,
                states: states.clone(),
            });
            let analyzer = Arc::new(FakeAnalyzer {
                database: self.paths.database_path.clone(),
                states: states.clone(),
                reject,
                mutate,
                calls: AtomicUsize::new(0),
            });
            (
                GenerationService::injected(
                    self.paths.clone(),
                    worker,
                    analyzer,
                    self.paths.resources_dir.join("fake-runtime"),
                    "runtime-test-v1".into(),
                ),
                states,
            )
        }
    }

    fn request(parent_job_id: Option<String>) -> CreateStudioMusicRequest {
        CreateStudioMusicRequest {
            activity: domain::Activity::DeepWork,
            sound_style_id: "ambient".into(),
            energy: StudioEnergy::Medium,
            duration_seconds: StudioDuration::Seconds90,
            note: Some("steady rain".into()),
            parent_job_id,
        }
    }

    fn wait_for(
        paths: &StudioRuntimePaths,
        id: &StudioJobId,
        expected: StudioJobState,
    ) -> StudioJobRecord {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut store = PreferencesRepository::open(&paths.database_path).unwrap();
            let record = store.load_studio_job(id).unwrap().unwrap();
            if record.state == expected {
                return record;
            }
            assert!(
                Instant::now() < deadline,
                "job remained in {:?}",
                record.state
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn success_follows_exact_states_and_persists_verified_artifact() {
        let harness = Harness::new();
        let (service, states) = harness.service(
            WorkerOutcome::Success,
            OutputMode::Draft,
            None,
            false,
            false,
        );
        let queued = service.create(request(None)).unwrap();
        assert_eq!(queued.state, StudioJobState::Queued);
        let ready = wait_for(&harness.paths, &queued.job_id, StudioJobState::Ready);
        assert_eq!(
            states.lock().unwrap().as_slice(),
            &[StudioJobState::Generating, StudioJobState::Analyzing]
        );
        let mut store = PreferencesRepository::open(&harness.paths.database_path).unwrap();
        let artifact = store
            .load_studio_job_artifact(&ready.job_id)
            .unwrap()
            .unwrap();
        assert_eq!(artifact.runtime_version, "runtime-test-v1");
        assert_eq!(artifact.stage, "ready");
        assert!(artifact.output_sha256.is_some());
        assert!(harness.paths.draft_output_path(&ready.job_id).is_file());
    }

    #[test]
    fn a_second_create_is_rejected_while_worker_is_active() {
        let harness = Harness::new();
        let gate = Gate::new();
        let (service, _) = harness.service(
            WorkerOutcome::Success,
            OutputMode::Draft,
            Some(gate.clone()),
            false,
            false,
        );
        let first = service.create(request(None)).unwrap();
        gate.wait_started();
        assert_eq!(service.create(request(None)).unwrap_err(), ACTIVE_MESSAGE);
        gate.release();
        wait_for(&harness.paths, &first.job_id, StudioJobState::Ready);
    }

    #[test]
    fn worker_exit_oom_and_timeout_have_stable_failed_codes() {
        for (outcome, code) in [
            (WorkerOutcome::UnexpectedExit, "unexpected_exit"),
            (WorkerOutcome::GpuOutOfMemory, "gpu_oom"),
            (WorkerOutcome::Timeout, "timeout"),
        ] {
            let harness = Harness::new();
            let (service, _) = harness.service(outcome, OutputMode::Missing, None, false, false);
            let queued = service.create(request(None)).unwrap();
            let failed = wait_for(&harness.paths, &queued.job_id, StudioJobState::Failed);
            assert_eq!(failed.failure.unwrap().detail, code);
        }
    }

    #[test]
    fn cancel_waits_for_worker_exit_and_cancelled_job_cannot_finalize() {
        let harness = Harness::new();
        let gate = Gate::new();
        let (service, _) = harness.service(
            WorkerOutcome::Success,
            OutputMode::Draft,
            Some(gate.clone()),
            false,
            false,
        );
        let queued = service.create(request(None)).unwrap();
        gate.wait_started();
        let cancelled = service.cancel(&queued.job_id).unwrap();
        assert!(gate.exited.load(Ordering::SeqCst));
        assert_eq!(cancelled.state, StudioJobState::Cancelled);
        gate.release();
        assert!(!harness.paths.draft_output_path(&queued.job_id).exists());
        assert_eq!(
            wait_for(&harness.paths, &queued.job_id, StudioJobState::Cancelled).state,
            StudioJobState::Cancelled
        );
    }

    #[test]
    fn analyzer_rejection_and_hash_substitution_never_become_ready() {
        for (reject, mutate) in [(true, false), (false, true)] {
            let harness = Harness::new();
            let (service, _) = harness.service(
                WorkerOutcome::Success,
                OutputMode::Draft,
                None,
                reject,
                mutate,
            );
            let queued = service.create(request(None)).unwrap();
            wait_for(&harness.paths, &queued.job_id, StudioJobState::Rejected);
            assert!(!harness.paths.draft_output_path(&queued.job_id).exists());
        }
    }

    #[test]
    fn missing_and_multiple_worker_outputs_fail_validation() {
        for output in [OutputMode::Missing, OutputMode::Multiple] {
            let harness = Harness::new();
            let (service, _) = harness.service(WorkerOutcome::Success, output, None, false, false);
            let queued = service.create(request(None)).unwrap();
            let failed = wait_for(&harness.paths, &queued.job_id, StudioJobState::Failed);
            assert_eq!(failed.failure.unwrap().detail, "output_invalid");
        }
    }

    #[test]
    fn startup_recovery_interrupts_running_and_removes_only_its_staging() {
        let harness = Harness::new();
        let mut store = PreferencesRepository::open(&harness.paths.database_path).unwrap();
        let input = StudioPromptInput::new(
            domain::Activity::DeepWork,
            StudioId::new("ambient").unwrap(),
            None,
            StudioEnergy::Medium,
            Vec::new(),
            None,
            None,
            StudioDuration::Seconds90,
        )
        .unwrap();
        let prompt = build_studio_prompt(&input, 7).unwrap();
        let queued = StudioJobRecord::new(
            StudioJobId::new("job_recoveryabcdefghijkl").unwrap(),
            StudioAttemptId::new("attempt_recoveryabcdefghijkl").unwrap(),
            input,
            prompt,
            1,
        )
        .unwrap();
        store.create_studio_job(&queued).unwrap();
        let generating = store
            .transition_studio_job(&queued.job_id, 0, StudioJobState::Generating, 2, None)
            .unwrap();
        fs::create_dir_all(harness.paths.job_output_dir(&queued.job_id)).unwrap();
        fs::write(
            harness.paths.job_output_dir(&queued.job_id).join("partial"),
            b"x",
        )
        .unwrap();
        fs::write(harness.paths.job_request_path(&queued.job_id), b"{}").unwrap();
        let retained = harness.paths.resources_dir.join("drafts/retained.flac");
        fs::create_dir_all(retained.parent().unwrap()).unwrap();
        fs::write(&retained, b"ready").unwrap();
        drop(store);
        let (service, _) = harness.service(
            WorkerOutcome::Success,
            OutputMode::Draft,
            None,
            false,
            false,
        );
        service.recover().unwrap();
        assert_eq!(
            wait_for(
                &harness.paths,
                &generating.job_id,
                StudioJobState::Interrupted
            )
            .state,
            StudioJobState::Interrupted
        );
        assert!(!harness.paths.job_output_dir(&queued.job_id).exists());
        assert!(!harness.paths.job_request_path(&queued.job_id).exists());
        assert!(retained.is_file());
    }

    #[test]
    fn regenerate_records_parent_with_new_job_attempt_and_seed() {
        let harness = Harness::new();
        let (service, _) = harness.service(
            WorkerOutcome::Success,
            OutputMode::Draft,
            None,
            false,
            false,
        );
        let first = service.create(request(None)).unwrap();
        let first = wait_for(&harness.paths, &first.job_id, StudioJobState::Ready);
        let second = service
            .create(request(Some(first.job_id.as_str().to_owned())))
            .unwrap();
        let second = wait_for(&harness.paths, &second.job_id, StudioJobState::Ready);
        assert_ne!(first.job_id, second.job_id);
        assert_ne!(first.attempt_id, second.attempt_id);
        assert_ne!(first.prompt.seed, second.prompt.seed);
        let mut store = PreferencesRepository::open(&harness.paths.database_path).unwrap();
        assert_eq!(
            store
                .load_studio_job_artifact(&second.job_id)
                .unwrap()
                .unwrap()
                .parent_job_id,
            Some(first.job_id)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_assignment_rejects_an_already_exited_process() {
        let mut child = Command::new("cmd")
            .args(["/C", "exit", "0"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        assert!(WindowsJob::assign(&child).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_termination_stops_and_reaps_the_assigned_process() {
        let mut child = Command::new("cmd")
            .args(["/C", "ping", "-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let job = WindowsJob::assign(&child).unwrap();
        job.terminate();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }
}
