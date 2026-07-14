use audio_engine::{
    decode_generated_draft_flac, AudioFacade, AudioIntensity, DecodedProgram, NativeAudioFacade,
    PlaybackSource, PlaybackState, SourceLabel,
};
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const ACTIVITIES: [&str; 5] = [
    "deep_work",
    "motivation",
    "creativity",
    "learning",
    "light_work",
];
const NEGATIVE: &str = "vocals, voice, lyrics, speech, spoken words, vocal samples, chanting, whispers, lead hooks, solos, drops, dramatic builds, sudden silence, abrupt transitions, tempo changes, cymbal crashes, sharp transients, alarms, bells, ringtones";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Create,
    Library,
    Batch,
    Package,
    Release,
    Jobs,
}

impl Page {
    const ALL: [(Page, &'static str); 6] = [
        (Page::Create, "＋  Create"),
        (Page::Library, "♫  Library"),
        (Page::Batch, "▦  Batch"),
        (Page::Package, "◇  Package"),
        (Page::Release, "↑  Release"),
        (Page::Jobs, "≡  Jobs"),
    ];
}

#[derive(Clone, Debug)]
struct Job {
    name: String,
    status: String,
    log: Vec<String>,
    completed: usize,
    total: usize,
    cancel: Arc<AtomicBool>,
    process_id: Option<u32>,
}

#[derive(Clone)]
struct ProgressSpec {
    records: PathBuf,
    total: usize,
}

#[derive(Clone)]
struct PreviewSummary {
    path: PathBuf,
    run_path: PathBuf,
    run_id: String,
    title: String,
}

#[derive(Clone, PartialEq, Eq)]
struct FileSignature {
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}

#[derive(Clone, PartialEq, Eq)]
struct PreviewSignature {
    master: Option<FileSignature>,
    record: Option<FileSignature>,
    analyzer: Option<FileSignature>,
    evidence: Option<FileSignature>,
}

#[derive(Clone)]
struct PreviewCacheEntry {
    signature: PreviewSignature,
    verified: bool,
}

struct PreviewPlayer {
    audio: NativeAudioFacade,
    selected: Option<PathBuf>,
    volume: u8,
}

impl PreviewPlayer {
    fn new() -> Self {
        let mut audio = NativeAudioFacade::new();
        let _ = audio.set_master_volume(70);
        Self {
            audio,
            selected: None,
            volume: 70,
        }
    }

    fn play(&mut self, preview: &PreviewSummary) -> Result<(), String> {
        let bytes = fs::metadata(&preview.path)
            .map_err(|error| error.to_string())?
            .len();
        if !verified_generated_master(&preview.run_path, &preview.path, bytes, None) {
            return Err(
                "The generated track or its evidence changed; refresh or regenerate it.".into(),
            );
        }
        if self.audio.state() != PlaybackState::Stopped {
            self.audio.stop().map_err(|error| error.to_string())?;
        }
        let track = decode_generated_draft_flac(
            preview.path.clone(),
            SourceLabel {
                pack_id: "music-admin-preview".into(),
                pack_title: "Music Admin preview".into(),
                item_id: preview.title.clone(),
                item_title: preview.title.clone(),
                variant_id: preview.run_id.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
        let program = DecodedProgram::new(vec![track]).map_err(|error| error.to_string())?;
        self.audio
            .start_with_source(PlaybackSource::Draft(program), AudioIntensity::Off)
            .map_err(|error| error.to_string())?;
        self.audio
            .set_master_volume(self.volume)
            .map_err(|error| error.to_string())?;
        self.selected = Some(preview.path.clone());
        Ok(())
    }

    fn toggle(&mut self) -> Result<(), String> {
        match self.audio.state() {
            PlaybackState::Playing => self.audio.pause(),
            PlaybackState::Paused => self.audio.resume(),
            PlaybackState::Stopped => return Err("Choose Play on a generated track first.".into()),
        }
        .map_err(|error| error.to_string())
    }

    fn stop(&mut self) -> Result<(), String> {
        if self.audio.state() != PlaybackState::Stopped {
            self.audio.stop().map_err(|error| error.to_string())?;
        }
        self.selected = None;
        Ok(())
    }

    fn set_volume(&mut self, volume: u8) -> Result<(), String> {
        self.volume = volume;
        self.audio
            .set_master_volume(volume)
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PlanSummary {
    file: String,
    batch: String,
    candidates: usize,
    activities: Vec<String>,
    tracks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RunSummary {
    id: String,
    batch: Option<String>,
    generated: usize,
}

#[derive(Clone)]
struct CandidateInput {
    id: String,
    activity: usize,
    genre: String,
    moods: String,
    duration: u64,
    bpm: u64,
    seed: u64,
    key: String,
    prompt: String,
    template: Option<Value>,
}

impl Default for CandidateInput {
    fn default() -> Self {
        Self {
            id: "deep-work-ambient-001".into(),
            activity: 0,
            genre: "ambient-electronic".into(),
            moods: "calm, steady".into(),
            duration: 180,
            bpm: 80,
            seed: 41_080_001,
            key: "C major".into(),
            prompt: "Warm instrumental ambient focus music, steady and subtle, without a foreground melody".into(),
            template: None,
        }
    }
}

struct AdminApp {
    root: PathBuf,
    page: Page,
    plans: Vec<PlanSummary>,
    runs: Vec<RunSummary>,
    jobs: Arc<Mutex<Vec<Job>>>,
    previews: Arc<Mutex<Vec<PreviewSummary>>>,
    preview_cache: Arc<Mutex<HashMap<PathBuf, PreviewCacheEntry>>>,
    preview_scan_running: Arc<AtomicBool>,
    preview_player: PreviewPlayer,
    preview_filter: String,
    last_preview_scan: Instant,
    message: String,
    selected_plan: usize,
    batch_id: String,
    notes: String,
    candidate: CandidateInput,
    batch_rows: String,
    run_id: String,
    pack_id: String,
    pack_title: String,
    flac_output: String,
    opus_output: String,
    flac_version: String,
    opus_version: String,
    app_requirement: String,
    release_tag: String,
}

impl AdminApp {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        configure_style(&context.egui_ctx);
        let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
        let version = workspace_version(&root).unwrap_or_else(|| "0.0.0".into());
        let mut app = Self {
            root,
            page: Page::Create,
            plans: Vec::new(),
            runs: Vec::new(),
            jobs: Arc::new(Mutex::new(Vec::new())),
            previews: Arc::new(Mutex::new(Vec::new())),
            preview_cache: Arc::new(Mutex::new(HashMap::new())),
            preview_scan_running: Arc::new(AtomicBool::new(false)),
            preview_player: PreviewPlayer::new(),
            preview_filter: String::new(),
            last_preview_scan: Instant::now() - Duration::from_secs(10),
            message: "Local production tools. Nothing is published automatically.".into(),
            selected_plan: 0,
            batch_id: "custom-focus-v1".into(),
            notes: "Draft created in Music Admin for internal review only.".into(),
            candidate: CandidateInput::default(),
            batch_rows: "motivation-pulse-001 | motivation | electronic | energetic, steady | 180 | 108 | 51080001 | A minor | Driving instrumental focus pulse".into(),
            run_id: "custom-focus-v1".into(),
            pack_id: "internal.custom-focus.v1".into(),
            pack_title: "Internal Custom Focus V1".into(),
            flac_output: ".local/pipeline/admin-flac-v1".into(),
            opus_output: ".local/pipeline/admin-opus-v1".into(),
            flac_version: "1.0.0-flac.1".into(),
            opus_version: "1.0.0-opus.1".into(),
            app_requirement: ">=0.2.1-beta.1, <0.3.0".into(),
            release_tag: format!("v{version}"),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.plans = scan_plans(&self.root).unwrap_or_default();
        self.runs = scan_runs(&self.root).unwrap_or_default();
        self.schedule_preview_scan();
        self.last_preview_scan = Instant::now();
        self.selected_plan = self.selected_plan.min(self.plans.len().saturating_sub(1));
    }

    fn schedule_preview_scan(&self) {
        if self.preview_scan_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let root = self.root.clone();
        let previews = Arc::clone(&self.previews);
        let cache = Arc::clone(&self.preview_cache);
        let running = Arc::clone(&self.preview_scan_running);
        thread::spawn(move || {
            if let Ok(mut cache) = cache.lock() {
                if let Ok(result) = scan_previews(&root, &mut cache) {
                    if let Ok(mut current) = previews.lock() {
                        *current = result;
                    }
                }
            }
            running.store(false, Ordering::Release);
        });
    }

    fn load_candidate_from_plan(&mut self, index: usize, candidate_id: &str) {
        let Some(plan) = self.plans.get(index) else {
            return;
        };
        match candidate_inputs_from_plan(&self.root.join("content/plans").join(&plan.file)) {
            Ok(candidates) if !candidates.is_empty() => {
                self.selected_plan = index;
                let Some(candidate) = candidates
                    .into_iter()
                    .find(|candidate| candidate.id == candidate_id)
                else {
                    self.message = "That track is no longer present in the selected plan.".into();
                    return;
                };
                self.candidate = candidate;
                self.batch_id = format!("{}-revision", plan.batch);
                self.page = Page::Create;
                self.message =
                    "Loaded the track into a new revision; the original plan remains unchanged."
                        .into();
            }
            Ok(_) => self.message = "That plan has no tracks to clone.".into(),
            Err(error) => self.message = error,
        }
    }

    fn load_batch_from_plan(&mut self) {
        let Some(plan) = self.plans.get(self.selected_plan) else {
            return;
        };
        match candidate_inputs_from_plan(&self.root.join("content/plans").join(&plan.file)) {
            Ok(candidates) => {
                self.batch_rows = candidates
                    .iter()
                    .map(candidate_row)
                    .collect::<Vec<_>>()
                    .join("\n");
                self.batch_id = format!("{}-revision", plan.batch);
                self.message = "Loaded the source tracks as an editable new batch revision.".into();
            }
            Err(error) => self.message = error,
        }
    }

    fn selected_plan_file(&self) -> Option<String> {
        self.plans
            .get(self.selected_plan)
            .map(|plan| plan.file.clone())
    }

    fn start_job(
        &mut self,
        name: &str,
        commands: Vec<Vec<String>>,
        progress: Option<ProgressSpec>,
    ) {
        let mut jobs = self.jobs.lock().expect("job lock");
        if jobs.iter().any(|job| job.status == "Running") {
            self.message = "Another production job is already running.".into();
            return;
        }
        jobs.push(Job {
            name: name.into(),
            status: "Running".into(),
            log: Vec::new(),
            completed: 0,
            total: progress.as_ref().map_or(0, |value| value.total),
            cancel: Arc::new(AtomicBool::new(false)),
            process_id: None,
        });
        let index = jobs.len() - 1;
        let cancel = Arc::clone(&jobs[index].cancel);
        drop(jobs);
        let root = self.root.clone();
        let shared = Arc::clone(&self.jobs);
        thread::spawn(move || run_commands(root, shared, index, commands, progress, cancel));
        self.page = Page::Jobs;
        self.message = format!("{name} started.");
    }

    fn cancel_job(&mut self, index: usize) {
        let process_id = {
            let mut jobs = self.jobs.lock().expect("job lock");
            let Some(job) = jobs.get_mut(index) else {
                return;
            };
            job.cancel.store(true, Ordering::Release);
            job.process_id
        };
        if let Some(process_id) = process_id {
            terminate_process_tree(process_id);
        }
        self.message = "Cancellation requested; waiting for the worker to exit safely.".into();
    }

    fn create_plan(&mut self, candidates: Vec<CandidateInput>, run_after: bool) {
        let Some(source) = self.selected_plan_file() else {
            self.message = "Choose a source plan first.".into();
            return;
        };
        match save_plan(
            &self.root,
            &source,
            &self.batch_id,
            &self.notes,
            &candidates,
        ) {
            Ok(file) => {
                self.run_id = self.batch_id.clone();
                self.message = format!("Saved {file}. Existing plans were not changed.");
                self.refresh();
                if run_after {
                    let total = candidates.len();
                    let records = self
                        .root
                        .join(".local/music-generation/runs")
                        .join(&self.run_id)
                        .join("generated-records");
                    self.start_job(
                        "Generate music",
                        vec![pipeline_command(
                            &self.root,
                            &[
                                "generate",
                                "--plan",
                                &format!("content/plans/{file}"),
                                "--run-id",
                                &self.run_id,
                            ],
                        )],
                        Some(ProgressSpec { records, total }),
                    );
                }
            }
            Err(error) => self.message = error,
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, eyebrow: &str, title: &str, description: &str) {
        ui.label(
            RichText::new(eyebrow.to_uppercase())
                .color(Color32::from_rgb(216, 165, 255))
                .small()
                .strong(),
        );
        ui.heading(RichText::new(title).size(30.0));
        ui.label(RichText::new(description).color(Color32::from_rgb(174, 163, 183)));
        ui.add_space(14.0);
    }

    fn plan_picker(&mut self, ui: &mut egui::Ui) {
        let selected = self
            .plans
            .get(self.selected_plan)
            .map(|plan| plan.batch.as_str())
            .unwrap_or("No plans found");
        egui::ComboBox::from_label("Start from")
            .selected_text(selected)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (index, plan) in self.plans.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected_plan,
                        index,
                        format!("{} · {} tracks", plan.batch, plan.candidates),
                    );
                }
            });
    }

    fn candidate_form(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            columns[0].label("Track ID");
            columns[0].text_edit_singleline(&mut self.candidate.id);
            columns[1].label("Activity");
            egui::ComboBox::from_id_salt("activity")
                .selected_text(ACTIVITIES[self.candidate.activity])
                .show_ui(&mut columns[1], |ui| {
                    for (index, activity) in ACTIVITIES.iter().enumerate() {
                        ui.selectable_value(&mut self.candidate.activity, index, *activity);
                    }
                });
            columns[0].label("Genre");
            columns[0].text_edit_singleline(&mut self.candidate.genre);
            columns[1].label("Moods (comma separated)");
            columns[1].text_edit_singleline(&mut self.candidate.moods);
            columns[0].label("Length");
            egui::ComboBox::from_id_salt("duration")
                .selected_text(format!("{} seconds", self.candidate.duration))
                .show_ui(&mut columns[0], |ui| {
                    ui.selectable_value(&mut self.candidate.duration, 90, "90 seconds");
                    ui.selectable_value(&mut self.candidate.duration, 180, "3 minutes");
                });
            columns[1].label("BPM");
            columns[1].add(egui::DragValue::new(&mut self.candidate.bpm).range(40..=200));
            columns[0].label("Seed");
            columns[0].add(egui::DragValue::new(&mut self.candidate.seed).range(0..=2_147_483_647));
            columns[1].label("Key");
            columns[1].text_edit_singleline(&mut self.candidate.key);
        });
        ui.label("Describe the music");
        ui.add(
            egui::TextEdit::multiline(&mut self.candidate.prompt)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
    }

    fn create_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Create",
            "Generate music",
            "One track with guarded production defaults.",
        );
        frame(ui, |ui| {
            self.plan_picker(ui);
            field(ui, "New revision / batch ID", &mut self.batch_id);
            self.candidate_form(ui);
            field(ui, "Notes", &mut self.notes);
            ui.horizontal(|ui| {
                if primary(ui, "Save draft plan").clicked() {
                    self.create_plan(vec![self.candidate.clone()], false);
                }
                if ui.button("Save & generate").clicked() {
                    self.create_plan(vec![self.candidate.clone()], true);
                }
            });
        });
    }

    fn library_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Library",
            "Plans & runs",
            "Update by cloning to a new revision; old evidence remains valid.",
        );
        ui.heading("Preview generated music");
        if self.preview_player.selected.is_some() {
            frame(ui, |ui| {
                let title = self
                    .preview_player
                    .selected
                    .as_ref()
                    .and_then(|path| path.file_stem())
                    .and_then(|value| value.to_str())
                    .unwrap_or("Generated track");
                ui.strong(format!("Now previewing: {title}"));
                ui.horizontal(|ui| {
                    let label = if self.preview_player.audio.state() == PlaybackState::Paused {
                        "Resume"
                    } else {
                        "Pause"
                    };
                    if primary(ui, label).clicked() {
                        if let Err(error) = self.preview_player.toggle() {
                            self.message = error;
                        }
                    }
                    if ui.button("Stop preview").clicked() {
                        if let Err(error) = self.preview_player.stop() {
                            self.message = error;
                        }
                    }
                    ui.label("Volume");
                    let mut volume = self.preview_player.volume;
                    if ui
                        .add(egui::Slider::new(&mut volume, 0..=100).show_value(true))
                        .changed()
                    {
                        if let Err(error) = self.preview_player.set_volume(volume) {
                            self.message = error;
                        }
                    }
                });
            });
            ui.add_space(8.0);
        }
        if self.previews.lock().expect("preview lock").is_empty() {
            frame(ui, |ui| {
                ui.label("Completed generated tracks will appear here automatically.");
            });
        }
        field(ui, "Find a track or run", &mut self.preview_filter);
        let filter = self.preview_filter.trim().to_ascii_lowercase();
        let previews = self
            .previews
            .lock()
            .expect("preview lock")
            .iter()
            .filter(|preview| {
                filter.is_empty()
                    || preview.title.to_ascii_lowercase().contains(&filter)
                    || preview.run_id.to_ascii_lowercase().contains(&filter)
            })
            .take(40)
            .cloned()
            .collect::<Vec<_>>();
        for preview in &previews {
            let selected = self.preview_player.selected.as_ref() == Some(&preview.path);
            frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.strong(&preview.title);
                        ui.small(format!("Run: {}", preview.run_id));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(if selected { "Restart" } else { "Play" })
                            .clicked()
                        {
                            if let Err(error) = self.preview_player.play(preview) {
                                self.message = format!("Preview failed: {error}");
                            }
                        }
                        if selected && ui.button("Stop").clicked() {
                            if let Err(error) = self.preview_player.stop() {
                                self.message = format!("Preview stop failed: {error}");
                            }
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
        ui.add_space(16.0);
        ui.heading("Plans");
        let plans = self.plans.clone();
        for (index, plan) in plans.iter().enumerate() {
            frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(&plan.batch);
                    ui.label(format!("{} tracks", plan.candidates));
                });
                ui.label(RichText::new(plan.activities.join(", ")).color(Color32::GRAY));
                ui.collapsing("Choose a track to update", |ui| {
                    for track in &plan.tracks {
                        if ui.button(format!("Edit {track}")).clicked() {
                            self.load_candidate_from_plan(index, track);
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }
        ui.add_space(10.0);
        ui.heading("Generation runs");
        for run in &self.runs {
            frame(ui, |ui| {
                ui.strong(&run.id);
                ui.label(format!(
                    "{} generated · {}",
                    run.generated,
                    run.batch.as_deref().unwrap_or("legacy run")
                ));
            });
            ui.add_space(6.0);
        }
    }

    fn batch_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Batch",
            "Generate a set",
            "One line per track; save and run them together.",
        );
        frame(ui, |ui| {
            self.plan_picker(ui);
            if ui.button("Load all source tracks to update").clicked() {
                self.load_batch_from_plan();
            }
            field(ui, "New revision / batch ID", &mut self.batch_id);
            ui.label("track ID | activity | genre | moods | seconds | BPM | seed | key | prompt");
            ui.add(
                egui::TextEdit::multiline(&mut self.batch_rows)
                    .desired_rows(9)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            field(ui, "Notes", &mut self.notes);
            ui.horizontal(|ui| {
                if primary(ui, "Save batch plan").clicked() {
                    match parse_batch_rows(&self.batch_rows) {
                        Ok(values) => self.create_plan(values, false),
                        Err(error) => self.message = error,
                    }
                }
                if ui.button("Save & generate batch").clicked() {
                    match parse_batch_rows(&self.batch_rows) {
                        Ok(values) => self.create_plan(values, true),
                        Err(error) => self.message = error,
                    }
                }
            });
        });
    }

    fn package_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Package",
            "Compress for the app",
            "Keep FLAC masters and create a separate smaller Opus pack.",
        );
        frame(ui, |ui| {
            self.plan_picker(ui);
            field(ui, "Run ID", &mut self.run_id);
            ui.columns(2, |columns| {
                field(&mut columns[0], "Pack ID", &mut self.pack_id);
                field(&mut columns[1], "Pack title", &mut self.pack_title);
                field(&mut columns[0], "FLAC version", &mut self.flac_version);
                field(&mut columns[1], "Opus version", &mut self.opus_version);
            });
            field(ui, "FLAC output", &mut self.flac_output);
            field(ui, "Opus output", &mut self.opus_output);
            field(ui, "Compatible app versions", &mut self.app_requirement);
            if primary(ui, "Build FLAC + Opus packs").clicked() {
                if let Some(plan) = self.selected_plan_file() {
                    let args = vec![
                        "package".into(),
                        "--plan".into(),
                        format!("content/plans/{plan}"),
                        "--run-id".into(),
                        self.run_id.clone(),
                        "--flac-output".into(),
                        self.flac_output.clone(),
                        "--opus-output".into(),
                        self.opus_output.clone(),
                        "--pack-id".into(),
                        self.pack_id.clone(),
                        "--pack-title".into(),
                        self.pack_title.clone(),
                        "--flac-version".into(),
                        self.flac_version.clone(),
                        "--opus-version".into(),
                        self.opus_version.clone(),
                        "--app-version-requirement".into(),
                        self.app_requirement.clone(),
                    ];
                    self.start_job(
                        "Package music",
                        vec![pipeline_owned_command(&self.root, &args)],
                        None,
                    );
                }
            }
        });
    }

    fn release_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Release",
            "Prepare safely",
            "Verify locally, then review and run the GitHub release command.",
        );
        frame(ui, |ui| {
            ui.heading("1. Verify everything");
            ui.label("Music tools, frontend, Rust formatting, Clippy, tests and builds.");
            if primary(ui, "Run full verification").clicked() {
                self.start_job("Full verification", verification_commands(), None);
            }
        });
        ui.add_space(10.0);
        frame(ui, |ui| {
            ui.heading("2. Check release identity");
            field(ui, "Release tag", &mut self.release_tag);
            if primary(ui, "Check release readiness").clicked() {
                self.start_job(
                    "Release readiness",
                    release_check_commands(&self.release_tag),
                    None,
                );
            }
        });
        ui.add_space(10.0);
        frame(ui, |ui| {
            ui.heading("3. Start signed beta after review");
            ui.label("The Admin never publishes automatically. Copy this only after the tag is created and pushed:");
            ui.code(format!(
                "gh workflow run public-beta.yml --ref {} -f release_tag={}",
                self.release_tag, self.release_tag
            ));
        });
    }

    fn jobs_page(&mut self, ui: &mut egui::Ui) {
        self.header(
            ui,
            "Activity",
            "Jobs",
            "Generation may take a while; logs update while it runs.",
        );
        let jobs = self.jobs.lock().expect("job lock").clone();
        if jobs.is_empty() {
            frame(ui, |ui| {
                ui.label("No jobs in this session.");
            });
        }
        for (index, job) in jobs.iter().enumerate().rev() {
            frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(&job.name);
                    let color = if job.status == "Succeeded" {
                        Color32::from_rgb(135, 221, 176)
                    } else if job.status == "Failed" {
                        Color32::from_rgb(235, 120, 145)
                    } else {
                        Color32::from_rgb(142, 229, 209)
                    };
                    ui.label(RichText::new(&job.status).color(color).strong());
                    if job.status == "Running" && ui.button("Cancel safely").clicked() {
                        self.cancel_job(index);
                    }
                });
                if job.total > 0 {
                    let progress = (job.completed as f32 / job.total as f32).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .show_percentage()
                            .text(format!(
                                "{} of {} tracks complete",
                                job.completed, job.total
                            )),
                    );
                    if job.status == "Running" {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Generating and validating the next track…");
                        });
                    }
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut job.log.join("\n").as_str())
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
            });
            ui.add_space(10.0);
        }
    }
}

impl eframe::App for AdminApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        context.request_repaint_after(std::time::Duration::from_millis(500));
        if self.last_preview_scan.elapsed() >= Duration::from_secs(2) {
            self.schedule_preview_scan();
            self.last_preview_scan = Instant::now();
        }
        egui::TopBottomPanel::top("top").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("A").color(Color32::from_rgb(216, 165, 255)));
                ui.vertical(|ui| {
                    ui.strong("Aria Music Admin");
                    ui.small("Native local production tool");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh();
                    }
                });
            });
            if !self.message.is_empty() {
                ui.label(RichText::new(&self.message).color(Color32::from_rgb(196, 184, 204)));
            }
        });
        egui::TopBottomPanel::bottom("navigation").show(context, |ui| {
            ui.horizontal(|ui| {
                for (page, label) in Page::ALL {
                    if ui.selectable_label(self.page == page, label).clicked() {
                        self.page = page;
                    }
                }
            });
        });
        egui::CentralPanel::default().show(context, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_max_width(760.0);
                match self.page {
                    Page::Create => self.create_page(ui),
                    Page::Library => self.library_page(ui),
                    Page::Batch => self.batch_page(ui),
                    Page::Package => self.package_page(ui),
                    Page::Release => self.release_page(ui),
                    Page::Jobs => self.jobs_page(ui),
                }
                ui.add_space(20.0);
            });
        });
    }
}

impl Drop for AdminApp {
    fn drop(&mut self) {
        let jobs = self.jobs.lock().expect("job lock").clone();
        for (index, job) in jobs.iter().enumerate() {
            if job.status == "Running" {
                self.cancel_job(index);
            }
        }
        let _ = self.preview_player.stop();
    }
}

fn configure_style(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(14, 11, 20);
    visuals.window_fill = Color32::from_rgb(24, 19, 31);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(31, 24, 40);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 35, 58);
    visuals.selection.bg_fill = Color32::from_rgb(98, 70, 122);
    context.set_visuals(visuals);
    context.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.button_padding = egui::vec2(13.0, 9.0);
    });
}

fn frame(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(Color32::from_rgb(23, 19, 31))
        .corner_radius(18)
        .inner_margin(16)
        .show(ui, content);
}

fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .color(Color32::from_rgb(25, 14, 32))
                .strong(),
        )
        .fill(Color32::from_rgb(216, 165, 255)),
    )
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn project_root() -> Result<PathBuf, String> {
    let mut path = std::env::current_exe().map_err(|error| error.to_string())?;
    while path.pop() {
        if path.join("Cargo.toml").is_file() && path.join("content/plans").is_dir() {
            return Ok(path);
        }
    }
    let path = std::env::current_dir().map_err(|error| error.to_string())?;
    if path.join("content/plans").is_dir() {
        Ok(path)
    } else {
        Err("Run Music Admin from the Aria Focus repository.".into())
    }
}

fn workspace_version(root: &Path) -> Option<String> {
    let value: Value = serde_json::from_slice(&fs::read(root.join("package.json")).ok()?).ok()?;
    value.get("version")?.as_str().map(str::to_owned)
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn scan_plans(root: &Path) -> Result<Vec<PlanSummary>, String> {
    let mut plans = Vec::new();
    for entry in fs::read_dir(root.join("content/plans")).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let value = read_json(&path)?;
        plans.push(PlanSummary {
            file: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .into(),
            batch: value
                .pointer("/batch/id")
                .and_then(Value::as_str)
                .unwrap_or("invalid plan")
                .into(),
            candidates: value
                .get("candidates")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            activities: value
                .pointer("/taxonomy/activities")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            tracks: value
                .get("candidates")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("id"))
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    plans.sort_by(|left, right| left.batch.cmp(&right.batch));
    Ok(plans)
}

fn scan_runs(root: &Path) -> Result<Vec<RunSummary>, String> {
    let directory = root.join(".local/music-generation/runs");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if !path.is_dir() {
            continue;
        }
        let identity = read_json(&path.join("plan-identity.json")).ok();
        let generated = fs::read_dir(path.join("generated-records"))
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0);
        runs.push(RunSummary {
            id: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .into(),
            batch: identity
                .as_ref()
                .and_then(|value| value.get("batch_id"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            generated,
        });
    }
    runs.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(runs)
}

fn scan_previews(
    root: &Path,
    cache: &mut HashMap<PathBuf, PreviewCacheEntry>,
) -> Result<Vec<PreviewSummary>, String> {
    let directory = root.join(".local/music-generation/runs");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut previews = Vec::new();
    for run in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let run = run.map_err(|error| error.to_string())?.path();
        if !run.is_dir() {
            continue;
        }
        let run_id = run
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        let masters = run.join("masters");
        let Ok(files) = fs::read_dir(masters) else {
            continue;
        };
        for file in files.filter_map(Result::ok) {
            let path = file.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() > 0
                && path.extension().and_then(|value| value.to_str()) == Some("flac")
                && verified_generated_master(&run, &path, metadata.len(), Some(cache))
            {
                previews.push(PreviewSummary {
                    title: path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("generated track")
                        .to_owned(),
                    path,
                    run_path: run.clone(),
                    run_id: run_id.clone(),
                });
            }
        }
    }
    previews.sort_by(|left, right| {
        left.run_id
            .cmp(&right.run_id)
            .then_with(|| left.title.cmp(&right.title))
    });
    Ok(previews)
}

fn verified_generated_master(
    run: &Path,
    master: &Path,
    bytes: u64,
    cache: Option<&mut HashMap<PathBuf, PreviewCacheEntry>>,
) -> bool {
    let Some(file_name) = master.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(candidate_id) = master.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let record_path = run
        .join("generated-records")
        .join(format!("{candidate_id}.json"));
    let Ok(record) = read_json(&record_path) else {
        return false;
    };
    let Some(analyzer_name) = record
        .pointer("/verified/analyzer_file_name")
        .and_then(Value::as_str)
    else {
        return false;
    };
    let Some(evidence_name) = record
        .pointer("/evidence/file_name")
        .and_then(Value::as_str)
    else {
        return false;
    };
    if Path::new(analyzer_name)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(analyzer_name)
        || Path::new(evidence_name)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(evidence_name)
    {
        return false;
    }
    let analyzer = run.join("analyzer-reports").join(analyzer_name);
    let evidence = run.join("generation-evidence").join(evidence_name);
    let signature = PreviewSignature {
        master: file_signature(master),
        record: file_signature(&record_path),
        analyzer: file_signature(&analyzer),
        evidence: file_signature(&evidence),
    };
    if let Some(entry) = cache.as_ref().and_then(|cache| cache.get(master)) {
        if entry.signature == signature {
            return entry.verified;
        }
    }
    let verified = record.get("schema").and_then(Value::as_str)
        == Some("adhd-music.candidate-ledger.generated")
        && record.get("schema_version").and_then(Value::as_u64) == Some(1)
        && record.get("lifecycle").and_then(Value::as_str) == Some("generated")
        && record.pointer("/candidate/id").and_then(Value::as_str) == Some(candidate_id)
        && record
            .pointer("/verified/file_name")
            .and_then(Value::as_str)
            == Some(file_name)
        && record.pointer("/verified/codec").and_then(Value::as_str) == Some("flac")
        && record.pointer("/verified/bytes").and_then(Value::as_u64) == Some(bytes)
        && record.pointer("/verified/sha256").and_then(Value::as_str)
            == hash_file(master).as_deref()
        && record
            .pointer("/verified/analyzer_sha256")
            .and_then(Value::as_str)
            == hash_file(&analyzer).as_deref()
        && record.pointer("/evidence/sha256").and_then(Value::as_str)
            == hash_file(&evidence).as_deref();
    if let Some(cache) = cache {
        cache.insert(
            master.to_owned(),
            PreviewCacheEntry {
                signature,
                verified,
            },
        );
    }
    verified
}

fn file_signature(path: &Path) -> Option<FileSignature> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return None;
    }
    Some(FileSignature {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn hash_file(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Some(format!("{:x}", digest.finalize()))
}

fn safe_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && ".-_".contains(character))
        })
    {
        Err(format!(
            "{label} must use letters, numbers, dots, hyphens, or underscores"
        ))
    } else {
        Ok(())
    }
}

fn nonempty(value: &str, label: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.trim().len() > max {
        Err(format!(
            "{label} is required and must be at most {max} characters"
        ))
    } else {
        Ok(())
    }
}

fn update_parameter(candidate: &mut Value, name: &str, value: &str) -> Result<(), String> {
    let parameters = candidate
        .pointer_mut("/inference/parameters")
        .and_then(Value::as_array_mut)
        .ok_or("Template inference parameters are invalid")?;
    let item = parameters
        .iter_mut()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("Template is missing {name}"))?;
    item["value"] = Value::String(value.into());
    Ok(())
}

fn plan_value(
    source: &Value,
    batch_id: &str,
    notes: &str,
    candidates: &[CandidateInput],
) -> Result<Value, String> {
    safe_id(batch_id, "Batch ID")?;
    nonempty(notes, "Notes", 1_000)?;
    if candidates.is_empty() || candidates.len() > 100 {
        return Err("A plan needs between 1 and 100 tracks".into());
    }
    let template = source
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or("Source plan has no candidate template")?;
    let mut ids = BTreeSet::new();
    let mut genres = BTreeSet::new();
    let mut moods = BTreeSet::new();
    let mut activities = BTreeSet::new();
    let mut output = Vec::new();
    for input in candidates {
        safe_id(&input.id, "Track ID")?;
        safe_id(&input.genre, "Genre")?;
        nonempty(&input.prompt, "Prompt", 2_000)?;
        nonempty(&input.key, "Key", 40)?;
        if !ids.insert(input.id.clone()) {
            return Err(format!("Duplicate track ID: {}", input.id));
        }
        if !matches!(input.duration, 90 | 180)
            || !(40..=200).contains(&input.bpm)
            || input.seed > 2_147_483_647
        {
            return Err(format!("{} has an invalid length, BPM, or seed", input.id));
        }
        let activity = *ACTIVITIES
            .get(input.activity)
            .ok_or("Unsupported activity")?;
        let track_moods: Vec<String> = input
            .moods
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        if track_moods.is_empty() || track_moods.len() > 4 {
            return Err(format!("{} needs one to four moods", input.id));
        }
        for mood in &track_moods {
            safe_id(mood, "Mood")?;
            moods.insert(mood.clone());
        }
        genres.insert(input.genre.clone());
        activities.insert(activity.to_owned());
        let mut candidate = input.template.clone().unwrap_or_else(|| template.clone());
        candidate["id"] = json!(input.id);
        candidate["seed"] = json!(input.seed);
        candidate["activity"] = json!(activity);
        candidate["genre_ids"] = json!([input.genre]);
        candidate["mood_ids"] = json!(track_moods);
        candidate["duration_seconds"] = json!(input.duration as f64);
        candidate["bpm"] = json!(input.bpm);
        candidate["contains_lyrics"] = json!(false);
        candidate["contains_speech"] = json!(false);
        let prompt = input.prompt.trim();
        let positive = if prompt.starts_with("[Instrumental]") {
            prompt.to_owned()
        } else {
            format!("[Instrumental] {prompt}")
        };
        candidate["prompts"] = json!({"positive": positive, "negative": NEGATIVE});
        update_parameter(&mut candidate, "keyscale", input.key.trim())?;
        output.push(candidate);
    }
    let mut batch = source
        .get("batch")
        .cloned()
        .ok_or("Source plan has no batch template")?;
    batch["id"] = json!(batch_id);
    batch["created_at"] = json!(utc_now());
    batch["notes"] = json!(notes.trim());
    Ok(json!({
        "schema": "adhd-music.candidate-ledger.planned", "schema_version": 1,
        "batch": batch,
        "taxonomy": {
            "activities": activities,
            "genres": genres.iter().map(|id| json!({"id":id,"label":label(id)})).collect::<Vec<_>>(),
            "moods": moods.iter().map(|id| json!({"id":id,"label":label(id)})).collect::<Vec<_>>()
        },
        "candidates": output
    }))
}

fn save_plan(
    root: &Path,
    source_file: &str,
    batch_id: &str,
    notes: &str,
    candidates: &[CandidateInput],
) -> Result<String, String> {
    safe_id(batch_id, "Batch ID")?;
    let plan_dir = root.join("content/plans");
    let source_path = plan_dir.join(source_file);
    if source_path.parent() != Some(plan_dir.as_path())
        || source_path.extension().and_then(|value| value.to_str()) != Some("json")
    {
        return Err("Source plan must be inside content/plans".into());
    }
    let plan = plan_value(&read_json(&source_path)?, batch_id, notes, candidates)?;
    let name = format!("{batch_id}.json");
    let output = plan_dir.join(&name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|_| {
            "That batch already exists. Use a new revision ID to preserve old evidence.".to_owned()
        })?;
    serde_json::to_writer_pretty(&mut file, &plan).map_err(|error| error.to_string())?;
    writeln!(file).map_err(|error| error.to_string())?;
    let validation = Command::new("cargo")
        .args([
            "run",
            "-q",
            "-p",
            "candidate-ledger",
            "--bin",
            "candidate-ledger",
            "--",
            "validate-plan",
            "--plan",
            &format!("content/plans/{name}"),
        ])
        .current_dir(root)
        .output();
    if !matches!(&validation, Ok(value) if value.status.success()) {
        let _ = fs::remove_file(&output);
        let detail = validation
            .map(|value| {
                let stderr = String::from_utf8_lossy(&value.stderr);
                let stdout = String::from_utf8_lossy(&value.stdout);
                format!("{stdout}\n{stderr}").trim().to_owned()
            })
            .unwrap_or_else(|error| error.to_string());
        return Err(format!(
            "Production validation rejected the new plan; the draft was removed. {detail}"
        ));
    }
    Ok(name)
}

fn parse_batch_rows(value: &str) -> Result<Vec<CandidateInput>, String> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() != 9 {
                return Err(format!("Line {} needs exactly 9 fields", index + 1));
            }
            let activity = ACTIVITIES
                .iter()
                .position(|value| *value == parts[1])
                .ok_or_else(|| format!("Line {} has an invalid activity", index + 1))?;
            Ok(CandidateInput {
                id: parts[0].into(),
                activity,
                genre: parts[2].into(),
                moods: parts[3].into(),
                duration: parts[4]
                    .parse()
                    .map_err(|_| format!("Line {} has an invalid length", index + 1))?,
                bpm: parts[5]
                    .parse()
                    .map_err(|_| format!("Line {} has an invalid BPM", index + 1))?,
                seed: parts[6]
                    .parse()
                    .map_err(|_| format!("Line {} has an invalid seed", index + 1))?,
                key: parts[7].into(),
                prompt: parts[8].into(),
                template: None,
            })
        })
        .collect()
}

fn candidate_inputs_from_plan(path: &Path) -> Result<Vec<CandidateInput>, String> {
    let value = read_json(path)?;
    value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or("Plan candidates are invalid")?
        .iter()
        .map(|candidate| {
            let activity_name = candidate
                .get("activity")
                .and_then(Value::as_str)
                .ok_or("Track activity is missing")?;
            let activity = ACTIVITIES
                .iter()
                .position(|value| *value == activity_name)
                .ok_or("Track activity is unsupported")?;
            let parameter = candidate
                .pointer("/inference/parameters")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|item| item.get("name").and_then(Value::as_str) == Some("keyscale"))
                })
                .and_then(|item| item.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("C major");
            Ok(CandidateInput {
                id: candidate
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("Track ID is missing")?
                    .into(),
                activity,
                genre: candidate
                    .get("genre_ids")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(Value::as_str)
                    .ok_or("Track genre is missing")?
                    .into(),
                moods: candidate
                    .get("mood_ids")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                duration: candidate
                    .get("duration_seconds")
                    .and_then(Value::as_f64)
                    .ok_or("Track duration is missing")? as u64,
                bpm: candidate
                    .get("bpm")
                    .and_then(Value::as_u64)
                    .ok_or("Track BPM is missing")?,
                seed: candidate
                    .get("seed")
                    .and_then(Value::as_u64)
                    .ok_or("Track seed is missing")?,
                key: parameter.into(),
                prompt: candidate
                    .pointer("/prompts/positive")
                    .and_then(Value::as_str)
                    .ok_or("Track prompt is missing")?
                    .into(),
                template: Some(candidate.clone()),
            })
        })
        .collect()
}

fn candidate_row(candidate: &CandidateInput) -> String {
    format!(
        "{} | {} | {} | {} | {} | {} | {} | {} | {}",
        candidate.id,
        ACTIVITIES[candidate.activity],
        candidate.genre,
        candidate.moods,
        candidate.duration,
        candidate.bpm,
        candidate.seed,
        candidate.key,
        candidate.prompt
    )
}

fn label(value: &str) -> String {
    value
        .split(['-', '_'])
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn utc_now() -> String {
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')",
        ])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
        .trim()
        .into()
}

fn pipeline_command(root: &Path, args: &[&str]) -> Vec<String> {
    let mut command = vec![
        "python".into(),
        root.join("tools/music-generation/music_pipeline.py")
            .display()
            .to_string(),
    ];
    command.extend(args.iter().map(|value| (*value).to_owned()));
    command
}
fn pipeline_owned_command(root: &Path, args: &[String]) -> Vec<String> {
    let mut command = vec![
        "python".into(),
        root.join("tools/music-generation/music_pipeline.py")
            .display()
            .to_string(),
    ];
    command.extend_from_slice(args);
    command
}

fn verification_commands() -> Vec<Vec<String>> {
    vec![
        vec![
            "python".into(),
            "-m".into(),
            "unittest".into(),
            "discover".into(),
            "-s".into(),
            "tools/music-generation".into(),
            "-p".into(),
            "test_*.py".into(),
        ],
        vec!["pnpm".into(), "verify".into()],
        vec![
            "cargo".into(),
            "fmt".into(),
            "--all".into(),
            "--".into(),
            "--check".into(),
        ],
        vec![
            "cargo".into(),
            "clippy".into(),
            "--workspace".into(),
            "--all-targets".into(),
            "--".into(),
            "-D".into(),
            "warnings".into(),
        ],
        vec!["cargo".into(), "test".into(), "--workspace".into()],
    ]
}
fn release_check_commands(tag: &str) -> Vec<Vec<String>> {
    let admin = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "music-admin".into());
    vec![
        vec![
            "python".into(),
            "scripts/verify_release_tag.py".into(),
            tag.into(),
        ],
        vec![admin, "--check-clean".into()],
        vec!["gh".into(), "auth".into(), "status".into()],
    ]
}

fn run_commands(
    root: PathBuf,
    shared: Arc<Mutex<Vec<Job>>>,
    index: usize,
    commands: Vec<Vec<String>>,
    progress: Option<ProgressSpec>,
    cancel: Arc<AtomicBool>,
) {
    let monitor_stop = Arc::new(AtomicBool::new(false));
    let monitor = progress.map(|progress| {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&monitor_stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                update_progress(&shared, index, &progress);
                thread::sleep(Duration::from_millis(400));
            }
            update_progress(&shared, index, &progress);
        })
    });
    let mut success = true;
    for command in commands {
        if cancel.load(Ordering::Acquire) {
            success = false;
            break;
        }
        if command.is_empty() {
            continue;
        }
        append_log(&shared, index, format!("> {}", command.join(" ")));
        let child = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            append_log(&shared, index, "Could not start command".into());
            success = false;
            break;
        };
        if let Ok(mut jobs) = shared.lock() {
            jobs[index].process_id = Some(child.id());
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let readers = [stdout.map(Stream::Stdout), stderr.map(Stream::Stderr)]
            .into_iter()
            .flatten()
            .map(|stream| {
                let shared = Arc::clone(&shared);
                thread::spawn(move || match stream {
                    Stream::Stdout(stream) => {
                        for line in BufReader::new(stream).lines().map_while(Result::ok) {
                            append_log(&shared, index, line);
                        }
                    }
                    Stream::Stderr(stream) => {
                        for line in BufReader::new(stream).lines().map_while(Result::ok) {
                            append_log(&shared, index, line);
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut cancelled = false;
        let status = loop {
            if cancel.load(Ordering::Acquire) {
                cancelled = true;
                terminate_process_tree(child.id());
                break child.wait().ok();
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(_) => break None,
            }
        };
        for reader in readers {
            let _ = reader.join();
        }
        if let Ok(mut jobs) = shared.lock() {
            jobs[index].process_id = None;
        }
        if cancelled || !matches!(status, Some(status) if status.success()) {
            success = false;
            break;
        }
    }
    monitor_stop.store(true, Ordering::Release);
    if let Some(monitor) = monitor {
        let _ = monitor.join();
    }
    if let Ok(mut jobs) = shared.lock() {
        if success && jobs[index].total > 0 {
            jobs[index].completed = jobs[index].total;
        }
        jobs[index].status = if cancel.load(Ordering::Acquire) {
            "Cancelled".into()
        } else if success {
            "Succeeded".into()
        } else {
            "Failed".into()
        };
    }
}

#[cfg(windows)]
fn terminate_process_tree(process_id: u32) {
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn terminate_process_tree(process_id: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &process_id.to_string()])
        .status();
}

fn verify_clean_worktree(root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("could not inspect repository state: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    clean_status(&output.stdout)
}

fn clean_status(status: &[u8]) -> Result<(), String> {
    if status.iter().all(u8::is_ascii_whitespace) {
        Ok(())
    } else {
        Err(
            "release readiness requires a clean repository (including staged and untracked files)"
                .into(),
        )
    }
}
fn update_progress(shared: &Arc<Mutex<Vec<Job>>>, index: usize, progress: &ProgressSpec) {
    let completed = fs::read_dir(&progress.records)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                })
                .count()
        })
        .unwrap_or(0)
        .min(progress.total);
    if let Ok(mut jobs) = shared.lock() {
        jobs[index].completed = completed;
    }
}
enum Stream {
    Stdout(std::process::ChildStdout),
    Stderr(std::process::ChildStderr),
}
fn append_log(shared: &Arc<Mutex<Vec<Job>>>, index: usize, line: String) {
    if let Ok(mut jobs) = shared.lock() {
        jobs[index].log.push(line);
        if jobs[index].log.len() > 2000 {
            jobs[index].log.remove(0);
        }
    }
}

fn main() -> eframe::Result {
    if std::env::args().nth(1).as_deref() == Some("--check-clean") {
        match project_root().and_then(|root| verify_clean_worktree(&root)) {
            Ok(()) => {
                println!("repository worktree: clean");
                return Ok(());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([840.0, 760.0])
            .with_min_inner_size([680.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Aria Music Admin",
        options,
        Box::new(|context| Ok(Box::new(AdminApp::new(context)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_batch_rows() {
        let values = parse_batch_rows(
            "one | deep_work | ambient | calm, steady | 180 | 80 | 42 | C major | quiet focus bed",
        )
        .unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].duration, 180);
    }
    #[test]
    fn rejects_bad_batch_rows() {
        assert!(parse_batch_rows("not enough | fields").is_err());
    }
    #[test]
    fn safe_ids_reject_paths() {
        assert!(safe_id("../escape", "ID").is_err());
        assert!(safe_id("good-v1", "ID").is_ok());
    }

    #[test]
    fn new_plan_clones_production_contract_without_mutating_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = read_json(&root.join("content/plans/deep-work-calibration-v1.json")).unwrap();
        let original = source.clone();
        let value = plan_value(
            &source,
            "admin-test-v1",
            "Internal test draft",
            &[CandidateInput::default()],
        )
        .unwrap();
        assert_eq!(
            value.pointer("/batch/id").and_then(Value::as_str),
            Some("admin-test-v1")
        );
        assert_eq!(
            value
                .get("candidates")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            1
        );
        assert!(value
            .pointer("/candidates/0/prompts/positive")
            .and_then(Value::as_str)
            .unwrap()
            .starts_with("[Instrumental]"));
        assert_eq!(source, original);
    }

    #[test]
    fn progress_counts_only_completed_json_records() {
        let directory =
            std::env::temp_dir().join(format!("aria-music-admin-progress-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("one.json"), b"{}").unwrap();
        fs::write(directory.join("partial.tmp"), b"partial").unwrap();
        let jobs = Arc::new(Mutex::new(vec![Job {
            name: "generate".into(),
            status: "Running".into(),
            log: Vec::new(),
            completed: 0,
            total: 2,
            cancel: Arc::new(AtomicBool::new(false)),
            process_id: None,
        }]));
        update_progress(
            &jobs,
            0,
            &ProgressSpec {
                records: directory.clone(),
                total: 2,
            },
        );
        assert_eq!(jobs.lock().unwrap()[0].completed, 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn release_cleanliness_rejects_every_porcelain_entry() {
        assert!(clean_status(b"").is_ok());
        assert!(clean_status(b"   \r\n").is_ok());
        assert!(clean_status(b" M tracked.rs\n").is_err());
        assert!(clean_status(b"A  staged.rs\n").is_err());
        assert!(clean_status(b"?? untracked.rs\n").is_err());
    }

    #[test]
    fn selected_candidate_template_preserves_hidden_inference_values() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = read_json(&root.join("content/plans/deep-work-calibration-v1.json")).unwrap();
        let mut template = source.pointer("/candidates/0").unwrap().clone();
        template["inference"]["shift"] = json!(7);
        let input = CandidateInput {
            template: Some(template),
            ..CandidateInput::default()
        };
        let value = plan_value(&source, "preserve-template-v1", "test", &[input]).unwrap();
        assert_eq!(
            value
                .pointer("/candidates/0/inference/shift")
                .and_then(Value::as_u64),
            Some(7)
        );
    }

    #[test]
    fn preview_requires_complete_untampered_evidence_chain() {
        let root =
            std::env::temp_dir().join(format!("aria-music-admin-preview-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let run = root.join(".local/music-generation/runs/run-one");
        for folder in [
            "masters",
            "generated-records",
            "analyzer-reports",
            "generation-evidence",
        ] {
            fs::create_dir_all(run.join(folder)).unwrap();
        }
        let master = run.join("masters/track-one.flac");
        let analyzer = run.join("analyzer-reports/track-one.json");
        let evidence = run.join("generation-evidence/track-one.json");
        fs::write(&master, b"fLaC-test").unwrap();
        fs::write(&analyzer, b"analysis").unwrap();
        fs::write(&evidence, b"evidence").unwrap();
        let mut cache = HashMap::new();
        assert!(scan_previews(&root, &mut cache).unwrap().is_empty());
        let record = json!({
            "schema":"adhd-music.candidate-ledger.generated", "schema_version":1,
            "lifecycle":"generated", "candidate":{"id":"track-one"},
            "verified":{"file_name":"track-one.flac","codec":"flac","bytes":9,
                "sha256":hash_file(&master).unwrap(),"analyzer_file_name":"track-one.json",
                "analyzer_sha256":hash_file(&analyzer).unwrap()},
            "evidence":{"file_name":"track-one.json","sha256":hash_file(&evidence).unwrap()}
        });
        fs::write(
            run.join("generated-records/track-one.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(scan_previews(&root, &mut cache).unwrap().len(), 1);
        let signature = cache.get(&master).unwrap().signature.clone();
        cache.insert(
            master.clone(),
            PreviewCacheEntry {
                signature,
                verified: false,
            },
        );
        assert!(scan_previews(&root, &mut cache).unwrap().is_empty());
        cache.clear();
        fs::write(&master, b"fLaC-evil").unwrap();
        assert!(scan_previews(&root, &mut cache).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_stops_a_running_process_tree() {
        let cancel = Arc::new(AtomicBool::new(false));
        let jobs = Arc::new(Mutex::new(vec![Job {
            name: "cancel-test".into(),
            status: "Running".into(),
            log: Vec::new(),
            completed: 0,
            total: 0,
            cancel: Arc::clone(&cancel),
            process_id: None,
        }]));
        #[cfg(windows)]
        let command = vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 30".into(),
        ];
        #[cfg(not(windows))]
        let command = vec!["sleep".into(), "30".into()];
        let worker_jobs = Arc::clone(&jobs);
        let worker_cancel = Arc::clone(&cancel);
        let worker = thread::spawn(move || {
            run_commands(
                std::env::current_dir().unwrap(),
                worker_jobs,
                0,
                vec![command],
                None,
                worker_cancel,
            )
        });
        for _ in 0..50 {
            if jobs.lock().unwrap()[0].process_id.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let process_id = jobs.lock().unwrap()[0].process_id.expect("child started");
        cancel.store(true, Ordering::Release);
        terminate_process_tree(process_id);
        worker.join().unwrap();
        assert_eq!(jobs.lock().unwrap()[0].status, "Cancelled");
    }
}
