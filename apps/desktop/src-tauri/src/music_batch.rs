use crate::{openrouter, secret_store};
use audio_analyzer::{analyze_file, Codec, DecodeStatus};
use fs2::FileExt;
use persistence::music_generation::{
    AttemptKind, BatchItemState, BatchState, MusicGenerationAttempt, MusicGenerationBatch,
    MusicGenerationBatchItem,
};
use persistence::PreferencesRepository;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

const DEFAULT_AUDIO_MODEL: &str = "google/lyria-3-pro-preview";
const DEFAULT_TEXT_MODEL: &str = "google/gemini-2.5-flash";
const DEFAULT_IMAGE_MODEL: &str = "google/gemini-2.5-flash-image";
const TEXT_INPUT_TOKENS_ESTIMATE: u64 = 1_200;
const TEXT_OUTPUT_TOKENS_ESTIMATE: u64 = 800;
// Lyria Pro returns complete couple-of-minute pieces rather than a strict
// container duration. Keep 180 seconds as the target while accepting the
// provider's observed 150–210 second range. This avoids discarding otherwise
// complete musical pieces solely because the provider rendered a shorter or
// longer arrangement than requested.
const PROVIDER_DURATION_TOLERANCE_RATIO: f64 = 0.10;
const PROVIDER_DURATION_TOLERANCE_SECONDS: f64 = 30.0;
const AUDIO_RETRY_ATTEMPTS: usize = 3;

fn cloud_mock_enabled() -> bool {
    cfg!(feature = "cloud-generation-mock")
}

#[cfg(feature = "cloud-generation-mock")]
fn mock_models() -> Vec<CloudModelDto> {
    let pricing = |request: &str| openrouter::ModelPricing {
        request: Some(request.into()),
        ..Default::default()
    };
    vec![
        CloudModelDto {
            id: "mock/audio".into(),
            name: Some("Test mock audio · no charge".into()),
            description: Some("Deterministic local fixture; never contacts OpenRouter.".into()),
            input_modalities: vec!["text".into()],
            output_modalities: vec!["audio".into()],
            supported_parameters: vec![],
            pricing: pricing("0"),
            context_length: Some(8_192),
            curated: true,
        },
        CloudModelDto {
            id: "mock/text".into(),
            name: Some("Test mock prompt · no charge".into()),
            description: Some("Deterministic local prompt fixture.".into()),
            input_modalities: vec!["text".into()],
            output_modalities: vec!["text".into()],
            supported_parameters: vec![],
            pricing: pricing("0"),
            context_length: Some(8_192),
            curated: true,
        },
        CloudModelDto {
            id: "mock/image".into(),
            name: Some("Test mock cover · no charge".into()),
            description: Some("Deterministic local SVG fixture.".into()),
            input_modalities: vec!["text".into()],
            output_modalities: vec!["image".into()],
            supported_parameters: vec![],
            pricing: pricing("0"),
            context_length: Some(8_192),
            curated: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CloudGenerationRequest {
    pub target_count: u16,
    pub activities: Vec<String>,
    pub audio_model: String,
    pub text_model: Option<String>,
    pub image_model: Option<String>,
    pub refine_prompts: bool,
    pub generate_covers: bool,
    pub duration_seconds: u16,
    pub budget_microdollars: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelDto {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub supported_parameters: Vec<String>,
    pub pricing: openrouter::ModelPricing,
    pub context_length: Option<u64>,
    pub curated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudKeyStatus {
    pub configured: bool,
    pub masked_suffix: Option<String>,
    pub mock: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudBatchSummary {
    pub batch_id: String,
    pub state: String,
    pub target_count: u16,
    pub completed_count: u16,
    pub failed_count: u16,
    pub reserved_microdollars: u64,
    pub actual_microdollars: u64,
    pub budget_microdollars: u64,
    pub activation_version: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudBatchItemDto {
    pub item_id: String,
    pub ordinal: u16,
    pub activity: String,
    pub state: String,
    pub audio_path: Option<String>,
    pub cover_path: Option<String>,
    pub cover_art: Option<String>,
    pub prompt_json: String,
    pub refined_prompt: Option<String>,
    pub audio_sha256: Option<String>,
    pub estimated_microdollars: u64,
    pub actual_microdollars: u64,
    pub validation_json: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudCostEstimate {
    pub target_count: u16,
    pub audio_microdollars: u64,
    pub text_microdollars: u64,
    pub image_microdollars: u64,
    pub total_microdollars: u64,
    pub currency: String,
    pub pricing_source: String,
}

pub(crate) struct CloudPreviewItem {
    pub(crate) path: PathBuf,
    pub(crate) codec: String,
    pub(crate) sha256: String,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: u16,
    pub(crate) bit_depth: Option<u16>,
    pub(crate) duration_seconds: f32,
    pub(crate) item_id: String,
    pub(crate) title: String,
}

/// The only file-backed contract shared with the playback service. This is
/// deliberately smaller than the generation database row and contains only
/// the validated, activated media needed for offline playback.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveCloudLibrary {
    pub(crate) version: String,
    pub(crate) batch_id: String,
    #[allow(dead_code)]
    pub(crate) activated_at_ms: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) previous_version: Option<String>,
    pub(crate) items: Vec<ActiveCloudItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveCloudItem {
    pub(crate) item_id: String,
    pub(crate) title: String,
    pub(crate) activity: String,
    pub(crate) audio_path: String,
    #[serde(default)]
    pub(crate) audio_codec: Option<String>,
    #[serde(default)]
    pub(crate) cover_path: Option<String>,
    #[serde(default)]
    pub(crate) cover_mime_type: Option<String>,
    #[serde(default)]
    pub(crate) cover_sha256: Option<String>,
    pub(crate) audio_sha256: String,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: u16,
    pub(crate) bit_depth: Option<u16>,
    pub(crate) duration_seconds: f32,
    #[serde(default)]
    pub(crate) genre_id: Option<String>,
    #[serde(default)]
    pub(crate) mood_id: Option<String>,
}

const MAX_ACTIVE_LIBRARY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CLOUD_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

/// Load and integrity-check the atomically activated cloud library. A
/// missing or invalid record is treated as unavailable by callers so the
/// existing offline catalogue remains a safe fallback.
pub(crate) fn load_active_library(root: &Path) -> Result<Option<ActiveCloudLibrary>, String> {
    let path = active_library_path(root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("active cloud library is not a regular file".into());
    }
    if metadata.len() > MAX_ACTIVE_LIBRARY_BYTES {
        return Err("active cloud library exceeds its size limit".into());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let library: ActiveCloudLibrary =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if library.version.trim().is_empty()
        || library.batch_id.trim().is_empty()
        || library.items.is_empty()
        || library.items.len() > 100
    {
        return Err("active cloud library metadata is invalid".into());
    }
    let mut ids = std::collections::HashSet::new();
    for item in &library.items {
        if item.item_id.trim().is_empty()
            || item.title.trim().is_empty()
            || item.activity.trim().is_empty()
            || !ids.insert(item.item_id.as_str())
            || item.sample_rate_hz == 0
            || !matches!(item.channels, 1 | 2)
            || !matches!(item.bit_depth, Some(16 | 24 | 32) | None)
            || item
                .audio_codec
                .as_deref()
                .is_some_and(|codec| !matches!(codec, "wav" | "flac" | "mp3"))
            || !item.duration_seconds.is_finite()
            || item.duration_seconds < 1.0
        {
            return Err("active cloud library contains invalid track metadata".into());
        }
        validate_cloud_media_path(root, &item.audio_path, MAX_CLOUD_MEDIA_BYTES)?;
        if let Some(cover) = &item.cover_path {
            let _ = item
                .cover_sha256
                .as_deref()
                .ok_or_else(|| "active cloud cover hash is missing".to_owned())?;
            validate_cloud_media_path(root, cover, catalogue::manifest::MAX_COVER_ART_BYTES)?;
        }
    }
    Ok(Some(library))
}

fn active_library_path(root: &Path) -> PathBuf {
    let current = root.join("cloud-generation").join("active.json");
    if current.exists() {
        return current;
    }
    // Versions before the content-root fix stored cloud media beside the
    // content directory. Keep that data readable while new batches use the
    // canonical content/cloud-generation location.
    root.parent()
        .map(|parent| parent.join("cloud-generation").join("active.json"))
        .filter(|legacy| legacy.exists())
        .unwrap_or(current)
}

fn validate_cloud_media_path(root: &Path, path: &str, max_bytes: u64) -> Result<(), String> {
    let path = Path::new(path);
    let original_metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if original_metadata.file_type().is_symlink() || !original_metadata.is_file() {
        return Err("activated cloud media is not a regular file".into());
    }
    let path_canonical = path.canonicalize().map_err(|error| error.to_string())?;
    let allowed_roots = [
        Some(root.join("cloud-generation")),
        root.parent().map(|parent| parent.join("cloud-generation")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|candidate| candidate.canonicalize().ok())
    .collect::<Vec<_>>();
    if !allowed_roots
        .iter()
        .any(|candidate| path_canonical.starts_with(candidate))
    {
        return Err("activated cloud media path escapes its storage root".into());
    }
    let metadata = fs::symlink_metadata(&path_canonical).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Err("activated cloud media is not a bounded regular file".into());
    }
    Ok(())
}

pub(crate) fn cloud_cover_data_url(root: &Path, item_id: &str) -> Result<Option<String>, String> {
    let Some(library) = load_active_library(root)? else {
        return Ok(None);
    };
    let Some(item) = library.items.iter().find(|item| item.item_id == item_id) else {
        return Ok(None);
    };
    let Some(path) = item.cover_path.as_deref() else {
        return Ok(None);
    };
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > catalogue::manifest::MAX_COVER_ART_BYTES {
        return Err("activated cloud cover exceeds its size limit".into());
    }
    let hash = sha256(&bytes);
    // Older records without a cover hash are intentionally not displayed.
    let expected = item
        .cover_sha256
        .as_deref()
        .ok_or_else(|| "activated cloud cover hash is missing".to_owned())?;
    if hash != expected {
        return Err("activated cloud cover failed its integrity check".into());
    }
    let mime = item.cover_mime_type.as_deref().unwrap_or("image/png");
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    Ok(Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

#[derive(Clone)]
pub struct CloudGenerationService {
    database: PathBuf,
    root: PathBuf,
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    models_cache: Arc<Mutex<Option<Vec<CloudModelDto>>>>,
}

/// Cross-process guard for the detached generation worker.
///
/// `running` prevents two workers in one process from overlapping, but it
/// cannot protect the shared preferences database when a developer CLI and
/// the desktop app are open together. Holding this OS file lock for the whole
/// worker lifetime keeps another process from incorrectly treating the batch
/// as an interrupted restart.
struct GenerationLease {
    _file: File,
}

impl CloudGenerationService {
    pub fn new(database: PathBuf, root: PathBuf) -> Self {
        Self {
            database,
            root,
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            models_cache: Arc::new(Mutex::new(None)),
        }
    }

    fn clear_models_cache(&self) {
        if let Ok(mut cache) = self.models_cache.lock() {
            *cache = None;
        }
    }

    fn generation_lock_path(&self) -> PathBuf {
        self.root.join("cloud-generation").join("generation.lock")
    }

    fn try_generation_lease(&self) -> Result<Option<GenerationLease>, String> {
        let directory = self.root.join("cloud-generation");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.generation_lock_path())
            .map_err(|error| error.to_string())?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(GenerationLease { _file: file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn reconcile_interrupted_batches(&self) -> Result<(), String> {
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        for batch in repo
            .list_music_generation_batches(50)
            .map_err(|e| e.to_string())?
        {
            if !matches!(
                batch.state,
                BatchState::Quoted
                    | BatchState::Authorized
                    | BatchState::Running
                    | BatchState::Paused
            ) {
                continue;
            }
            repo.update_music_generation_batch(
                &batch.batch_id,
                batch.revision,
                BatchState::Failed,
                batch.reserved_microdollars,
                batch.actual_microdollars,
                None,
                None,
                Some((
                    "interrupted_restart",
                    "Generation stopped when Aria Focus closed. Start a new batch to retry.",
                )),
                now_ms(),
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn key_status(&self) -> Result<CloudKeyStatus, String> {
        let mock = cloud_mock_enabled();
        let key = if mock {
            None
        } else {
            secret_store::load().map_err(|e| e.to_string())?
        };
        Ok(CloudKeyStatus {
            configured: mock || key.is_some(),
            masked_suffix: if mock {
                Some("TEST".into())
            } else {
                key.map(|key| {
                    key.chars()
                        .rev()
                        .take(4)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect()
                })
            },
            mock,
        })
    }

    pub fn save_key(&self, key: String) -> Result<CloudKeyStatus, String> {
        secret_store::save(&key).map_err(|e| e.to_string())?;
        self.clear_models_cache();
        self.key_status()
    }

    pub fn remove_key(&self) -> Result<CloudKeyStatus, String> {
        secret_store::remove().map_err(|e| e.to_string())?;
        self.clear_models_cache();
        self.key_status()
    }

    pub fn list_models(&self) -> Result<Vec<CloudModelDto>, String> {
        #[cfg(feature = "cloud-generation-mock")]
        if cloud_mock_enabled() {
            return Ok(mock_models());
        }
        if let Ok(cache) = self.models_cache.lock() {
            if let Some(models) = cache.as_ref() {
                return Ok(models.clone());
            }
        }
        let key = secret_store::load()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Add an OpenRouter API key before loading models.".to_owned())?;
        let models = openrouter::OpenRouterClient::new()
            .map_err(|e| e.message)?
            .list_models(&key)
            .map_err(|e| e.message)?;
        let models = models
            .into_iter()
            .filter(|model| {
                model
                    .output_modalities
                    .iter()
                    .any(|m| matches!(m.as_str(), "audio" | "image" | "text"))
                    || model.id == DEFAULT_AUDIO_MODEL
                    || model.id == DEFAULT_TEXT_MODEL
                    || model.id == DEFAULT_IMAGE_MODEL
            })
            .map(|model| {
                let model_id = model.id.clone();
                CloudModelDto {
                    curated: [DEFAULT_AUDIO_MODEL, DEFAULT_TEXT_MODEL, DEFAULT_IMAGE_MODEL]
                        .contains(&model_id.as_str()),
                    id: model.id,
                    name: model.name,
                    description: model.description,
                    input_modalities: model.input_modalities,
                    output_modalities: model.output_modalities,
                    supported_parameters: model.supported_parameters,
                    pricing: published_media_pricing(&model_id, model.pricing),
                    context_length: model.context_length,
                }
            })
            .collect::<Vec<_>>();
        if let Ok(mut cache) = self.models_cache.lock() {
            *cache = Some(models.clone());
        }
        Ok(models)
    }

    pub fn validate_key(&self, key: String) -> Result<CloudKeyStatus, String> {
        if cloud_mock_enabled() {
            return self.key_status();
        }
        let client = openrouter::OpenRouterClient::new().map_err(|e| e.message)?;
        client.validate_key(&key).map_err(|e| e.message)?;
        self.save_key(key)
    }

    pub fn estimate(&self, request: &CloudGenerationRequest) -> Result<CloudCostEstimate, String> {
        validate_request(request)?;
        if !self.key_status()?.configured {
            return Err("Add an OpenRouter API key before estimating cloud generation.".into());
        }
        let available = self.list_models()?;
        Self::estimate_with_models(request, &available)
    }

    fn estimate_with_models(
        request: &CloudGenerationRequest,
        available: &[CloudModelDto],
    ) -> Result<CloudCostEstimate, String> {
        let count = u64::from(request.target_count);
        let audio_per_track = model_cost(available, &request.audio_model, ModelCostKind::Audio)?;
        let text_per_track = if request.refine_prompts {
            let model = request
                .text_model
                .as_deref()
                .ok_or_else(|| "Choose a prompt model or disable prompt refinement.".to_owned())?;
            model_cost(available, model, ModelCostKind::Text)?
        } else {
            0
        };
        let image_per_track = if request.generate_covers {
            let model = request
                .image_model
                .as_deref()
                .ok_or_else(|| "Choose a cover model or disable cover generation.".to_owned())?;
            model_cost(available, model, ModelCostKind::Image)?
        } else {
            0
        };
        let audio = audio_per_track.saturating_mul(count);
        let text = text_per_track.saturating_mul(count);
        let image = image_per_track.saturating_mul(count);
        Ok(CloudCostEstimate {
            target_count: request.target_count,
            audio_microdollars: audio,
            text_microdollars: text,
            image_microdollars: image,
            total_microdollars: audio.saturating_add(text).saturating_add(image),
            currency: "USD".into(),
            pricing_source: "OpenRouter model pricing + published media rates".into(),
        })
    }

    pub fn create_batch(
        &self,
        request: CloudGenerationRequest,
    ) -> Result<CloudBatchSummary, String> {
        let Some(lease) = self.try_generation_lease()? else {
            return Err(
                "A cloud generation batch is already running in another Aria Focus process.".into(),
            );
        };
        self.reconcile_interrupted_batches()?;
        validate_request(&request)?;
        if !self.key_status()?.configured {
            return Err("Add an OpenRouter API key before starting cloud generation.".into());
        }
        let available = self.list_models()?;
        let audio_supported = available.iter().any(|model| {
            model.id == request.audio_model
                && (model.output_modalities.is_empty()
                    || model
                        .output_modalities
                        .iter()
                        .any(|modality| modality == "audio"))
        }) || request.audio_model == DEFAULT_AUDIO_MODEL;
        if !audio_supported {
            return Err("The selected audio model is unavailable or is not audio-capable.".into());
        }
        if let Some(model) = &request.text_model {
            if request.refine_prompts
                && !available.iter().any(|candidate| {
                    candidate.id == *model
                        && (candidate.output_modalities.is_empty()
                            || candidate
                                .output_modalities
                                .iter()
                                .any(|modality| modality == "text"))
                })
            {
                return Err("The selected prompt model is unavailable.".into());
            }
        }
        if let Some(model) = &request.image_model {
            if request.generate_covers
                && !available.iter().any(|candidate| {
                    candidate.id == *model
                        && (candidate.output_modalities.is_empty()
                            || candidate
                                .output_modalities
                                .iter()
                                .any(|modality| modality == "image"))
                })
            {
                return Err("The selected cover model is unavailable.".into());
            }
        }
        let estimate = Self::estimate_with_models(&request, &available)?;
        if request.budget_microdollars < estimate.total_microdollars {
            return Err(format!(
                "The selected budget is too low. Estimated maximum is ${:.2}.",
                estimate.total_microdollars as f64 / 1_000_000.0
            ));
        }
        if self.running.swap(true, Ordering::SeqCst) {
            drop(lease);
            return Err("A cloud generation batch is already running.".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        let batch_id = format!("cloud_batch_{}", unique_suffix());
        let now = now_ms();
        let activities = normalized_activities(&request.activities);
        let mut items = Vec::with_capacity(usize::from(request.target_count));
        for ordinal in 0..request.target_count {
            let activity = activities[usize::from(ordinal) % activities.len()].to_owned();
            let prompt = build_prompt(&activity, ordinal, &request);
            items.push(MusicGenerationBatchItem {
                item_id: format!("cloud_item_{}_{}", batch_id, ordinal),
                batch_id: batch_id.clone(),
                ordinal,
                activity,
                idempotency_key: format!("{}-{}", batch_id, ordinal),
                state: BatchItemState::Queued,
                prompt_json: serde_json::to_string(&prompt)
                    .map_err(|_| "The generation prompt could not be saved.".to_owned())?,
                refined_prompt: None,
                audio_model: request.audio_model.clone(),
                text_model: if request.refine_prompts {
                    request.text_model.clone()
                } else {
                    None
                },
                image_model: if request.generate_covers {
                    request.image_model.clone()
                } else {
                    None
                },
                audio_request_id: None,
                text_request_id: None,
                image_request_id: None,
                audio_path: None,
                cover_path: None,
                audio_sha256: None,
                cover_sha256: None,
                estimated_microdollars: estimate.total_microdollars
                    / u64::from(request.target_count),
                actual_microdollars: 0,
                validation_json: None,
                error_code: None,
                error_message: None,
                revision: 0,
                created_at_ms: now,
                updated_at_ms: now,
            });
        }
        let batch = MusicGenerationBatch { batch_id:batch_id.clone(),state:BatchState::Quoted,target_count:request.target_count,budget_microdollars:request.budget_microdollars,reserved_microdollars:0,actual_microdollars:0,catalog_snapshot_json:serde_json::to_string(&json!({"audio_model":request.audio_model,"text_model":request.text_model,"image_model":request.image_model,"duration_seconds":request.duration_seconds,"created_at_ms":now})).unwrap_or_else(|_|"{}".into()),activation_version:None,previous_activation_version:None,revision:0,created_at_ms:now,updated_at_ms:now,error_code:None,error_message:None };
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        if let Err(error) = repo.create_music_generation_batch(&batch, &items) {
            self.running.store(false, Ordering::SeqCst);
            drop(lease);
            return Err(error.to_string());
        }
        let service = self.clone();
        let request_for_thread = request.clone();
        thread::spawn(move || {
            service.run_batch(batch_id, request_for_thread, lease);
        });
        Ok(summary_from(&batch, &items))
    }

    pub fn cancel_batch(&self) -> Result<(), String> {
        self.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn resume_batch(
        &self,
        batch_id: &str,
        request: CloudGenerationRequest,
    ) -> Result<CloudBatchSummary, String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("A cloud generation batch is already running.".into());
        }
        let Some(lease) = self.try_generation_lease()? else {
            return Err(
                "A cloud generation batch is already running in another Aria Focus process.".into(),
            );
        };
        validate_request(&request)?;
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let mut batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        if batch.target_count != request.target_count {
            return Err("The retry count must match the original cloud batch.".into());
        }
        if !matches!(batch.state, BatchState::Failed | BatchState::Cancelled) {
            return Err("Only a failed or cancelled cloud batch can be resumed.".into());
        }
        let estimate = self.estimate(&request)?;
        let estimated_per_item = estimate
            .total_microdollars
            .saturating_add(u64::from(request.target_count).saturating_sub(1))
            / u64::from(request.target_count);
        if request.budget_microdollars > batch.budget_microdollars {
            batch = repo
                .update_music_generation_budget(
                    batch_id,
                    batch.revision,
                    request.budget_microdollars,
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
        }
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        for item in &items {
            if !matches!(
                item.state,
                BatchItemState::Validated | BatchItemState::Activated
            ) {
                self.remove_retry_outputs(batch_id, item.ordinal)?;
                repo.reset_music_generation_item_for_retry(
                    &item.item_id,
                    item.revision,
                    estimated_per_item,
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        let spent = repo
            .sum_music_generation_attempt_costs(batch_id)
            .map_err(|e| e.to_string())?;
        let batch = repo
            .update_music_generation_batch(
                batch_id,
                batch.revision,
                BatchState::Quoted,
                0,
                spent,
                None,
                None,
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
        self.cancel.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        let service = self.clone();
        let request_for_thread = request;
        let batch_id_for_thread = batch_id.to_owned();
        thread::spawn(move || {
            service.run_batch(batch_id_for_thread, request_for_thread, lease);
        });
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        Ok(summary_from(&batch, &items))
    }

    fn remove_retry_outputs(&self, batch_id: &str, ordinal: u16) -> Result<(), String> {
        let root = self
            .root
            .join("cloud-generation")
            .join("batches")
            .join(batch_id)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let prefix = format!("{ordinal}-");
        for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let candidate = entry.path();
            let name = candidate
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !name.starts_with(&prefix) {
                continue;
            }
            let metadata = fs::symlink_metadata(&candidate).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("A retry output is linked or is not a regular file.".into());
            }
            let resolved = candidate.canonicalize().map_err(|e| e.to_string())?;
            if !resolved.starts_with(&root) {
                return Err("A retry output is outside the cloud batch.".into());
            }
            fs::remove_file(resolved).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// A detached generation worker cannot be resumed safely after the app has
    /// been closed. Reconcile those durable in-flight rows before presenting
    /// them as active, so the UI can offer a clean retry instead of showing a
    /// batch that has no worker anymore.
    fn recover_interrupted_batches(&self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Another process may own the worker. In that case its durable rows
        // are still live and must not be rewritten as an interrupted restart.
        let Some(_lease) = self.try_generation_lease()? else {
            return Ok(());
        };
        self.reconcile_interrupted_batches()?;
        Ok(())
    }

    fn mark_batch_cancelled(
        &self,
        repo: &mut PreferencesRepository,
        batch_id: &str,
    ) -> Result<(), String> {
        let batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        for item in repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?
        {
            if matches!(
                item.state,
                BatchItemState::Queued
                    | BatchItemState::Refining
                    | BatchItemState::GeneratingAudio
                    | BatchItemState::GeneratingCover
                    | BatchItemState::Validating
            ) {
                repo.update_music_generation_item(
                    &item.item_id,
                    item.revision,
                    BatchItemState::Cancelled,
                    None,
                    (None, None, None),
                    (None, None),
                    (None, None),
                    item.actual_microdollars,
                    None,
                    Some(("cancelled", "Generation was cancelled by the user.")),
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        repo.update_music_generation_batch(
            batch_id,
            batch.revision,
            BatchState::Cancelled,
            batch.reserved_microdollars,
            batch.actual_microdollars,
            None,
            None,
            None,
            now_ms(),
        )
        .map_err(|e| e.to_string())?;
        if safe_batch_id(batch_id) {
            let staging = self
                .root
                .join("cloud-generation")
                .join("batches")
                .join(batch_id);
            let _ = fs::remove_dir_all(staging);
        }
        Ok(())
    }

    pub fn get_batch(&self, batch_id: &str) -> Result<Option<CloudBatchSummary>, String> {
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let Some(batch) = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        Ok(Some(summary_from(&batch, &items)))
    }

    pub fn get_active_batch(&self) -> Result<Option<CloudBatchSummary>, String> {
        self.recover_interrupted_batches()?;
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let mut batch = None;
        for candidate in repo
            .list_music_generation_batches(50)
            .map_err(|e| e.to_string())?
        {
            if !matches!(
                candidate.state,
                BatchState::Quoted
                    | BatchState::Authorized
                    | BatchState::Running
                    | BatchState::Paused
                    | BatchState::Validated
            ) {
                continue;
            }
            if !cloud_mock_enabled() && self.is_mock_batch(&mut repo, &candidate.batch_id)? {
                // A developer may have previously run the opt-in mock feature
                // against the same preferences database. Never present that
                // fixture as a real OpenRouter batch in a production build.
                continue;
            }
            batch = Some(candidate);
            break;
        }
        let Some(batch) = batch else {
            return Ok(None);
        };
        let items = repo
            .list_music_generation_items(&batch.batch_id)
            .map_err(|e| e.to_string())?;
        Ok(Some(summary_from(&batch, &items)))
    }

    /// Returns the most recently updated batch, including terminal failures.
    /// The event bridge uses this to publish a useful recovery state after a
    /// provider error instead of silently disappearing from the UI.
    pub fn get_latest_batch(&self) -> Result<Option<CloudBatchSummary>, String> {
        self.recover_interrupted_batches()?;
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let Some(batch) = repo
            .list_music_generation_batches(1)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        if !cloud_mock_enabled() && self.is_mock_batch(&mut repo, &batch.batch_id)? {
            return Ok(None);
        }
        let items = repo
            .list_music_generation_items(&batch.batch_id)
            .map_err(|e| e.to_string())?;
        Ok(Some(summary_from(&batch, &items)))
    }

    fn is_mock_batch(
        &self,
        repo: &mut PreferencesRepository,
        batch_id: &str,
    ) -> Result<bool, String> {
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        Ok(!items.is_empty()
            && items.iter().any(|item| {
                item.validation_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                    .and_then(|value| value.get("mock").and_then(Value::as_bool))
                    == Some(true)
            }))
    }

    pub fn get_items(&self, batch_id: &str) -> Result<Vec<CloudBatchItemDto>, String> {
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        repo.list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|item| {
                let cover_art = self.cover_data_url_for_item(&item)?;
                Ok(item_dto(item, cover_art))
            })
            .collect()
    }

    fn cover_data_url_for_item(
        &self,
        item: &MusicGenerationBatchItem,
    ) -> Result<Option<String>, String> {
        let Some(path) = item.cover_path.as_deref() else {
            return Ok(None);
        };
        validate_cloud_media_path(&self.root, path, catalogue::manifest::MAX_COVER_ART_BYTES)?;
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if let Some(expected) = item.cover_sha256.as_deref() {
            if sha256(&bytes) != expected {
                return Err("The generated cover failed its integrity check.".into());
            }
        }
        let mime = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| match extension.to_ascii_lowercase().as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "svg" => "image/svg+xml",
                _ => "image/png",
            })
            .unwrap_or("image/png");
        use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
        Ok(Some(format!(
            "data:{mime};base64,{}",
            BASE64_STANDARD.encode(bytes)
        )))
    }

    pub(crate) fn preview_item(
        &self,
        batch_id: &str,
        item_id: &str,
    ) -> Result<CloudPreviewItem, String> {
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        if !matches!(batch.state, BatchState::Validated | BatchState::Activated) {
            return Err("Preview becomes available after the track passes validation.".into());
        }
        let item = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|item| item.item_id == item_id)
            .ok_or_else(|| "The cloud batch item could not be found.".to_owned())?;
        if !matches!(
            item.state,
            BatchItemState::Validated | BatchItemState::Activated
        ) {
            return Err("This track is not ready to preview yet.".into());
        }
        let path = item
            .audio_path
            .ok_or_else(|| "The validated track has no audio output.".to_owned())?;
        let expected_hash = item
            .audio_sha256
            .ok_or_else(|| "The validated track has no integrity hash.".to_owned())?;
        validate_cloud_media_path(&self.root, &path, MAX_CLOUD_MEDIA_BYTES)?;
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if sha256(&bytes) != expected_hash {
            return Err("The preview audio failed its integrity check.".into());
        }
        let metadata = analyze_cloud_audio(Path::new(&path))?;
        Ok(CloudPreviewItem {
            path: PathBuf::from(path),
            codec: metadata.codec.to_owned(),
            sha256: expected_hash,
            sample_rate_hz: metadata.sample_rate_hz,
            channels: metadata.channels,
            bit_depth: metadata.bit_depth,
            duration_seconds: metadata.duration_seconds,
            title: format!(
                "{} soundscape {}",
                item.activity.replace('_', " "),
                item.ordinal.saturating_add(1)
            ),
            item_id: item.item_id,
        })
    }

    pub fn restore_previous(&self) -> Result<(), String> {
        let active = active_library_path(&self.root);
        let current: Value = serde_json::from_slice(
            &fs::read(&active)
                .map_err(|_| "No activated cloud library is available to restore.".to_owned())?,
        )
        .map_err(|_| "The activated cloud library record is corrupt.".to_owned())?;
        let previous = current
            .get("previous_version")
            .and_then(Value::as_str)
            .ok_or_else(|| "There is no previous cloud library to restore.".to_owned())?;
        let history = self
            .root
            .join("cloud-generation")
            .join("history")
            .join(format!("{previous}.json"));
        let previous_bytes = fs::read(&history)
            .map_err(|_| "The previous cloud library record is unavailable.".to_owned())?;
        let temp = active.with_extension("json.tmp");
        fs::write(&temp, previous_bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp, &active).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), String> {
        let canonical = self.root.join("cloud-generation").join("active.json");
        let legacy = self
            .root
            .parent()
            .map(|parent| parent.join("cloud-generation").join("active.json"));
        for path in [Some(canonical), legacy].into_iter().flatten() {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err("active cloud library pointer must not be a symlink".into());
                }
                Ok(metadata) if metadata.is_file() => {
                    fs::remove_file(path).map_err(|e| e.to_string())?;
                }
                Ok(_) => return Err("active cloud library pointer is not a regular file".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }

    fn run_batch(
        &self,
        batch_id: String,
        request: CloudGenerationRequest,
        _lease: GenerationLease,
    ) {
        let result = self.run_batch_inner(&batch_id, &request);
        if let Err(error) = result {
            if let Ok(mut repo) = PreferencesRepository::open(&self.database) {
                if self.cancel.load(Ordering::SeqCst) {
                    let _ = self.mark_batch_cancelled(&mut repo, &batch_id);
                } else if let Ok(Some(batch)) = repo.load_music_generation_batch(&batch_id) {
                    let _ = repo.update_music_generation_batch(
                        &batch_id,
                        batch.revision,
                        BatchState::Failed,
                        batch.reserved_microdollars,
                        batch.actual_microdollars,
                        None,
                        None,
                        Some(("batch_failed", &error)),
                        now_ms(),
                    );
                }
            }
        }
        self.running.store(false, Ordering::SeqCst);
    }

    fn run_batch_inner(
        &self,
        batch_id: &str,
        request: &CloudGenerationRequest,
    ) -> Result<(), String> {
        #[cfg(feature = "cloud-generation-mock")]
        if cloud_mock_enabled() {
            return self.run_mock_batch(batch_id, request);
        }
        let key = secret_store::load()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The OpenRouter API key is no longer available.".to_owned())?;
        let client = openrouter::OpenRouterClient::new().map_err(|e| e.message)?;
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let mut batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        batch = repo
            .update_music_generation_batch(
                batch_id,
                batch.revision,
                BatchState::Running,
                batch.reserved_microdollars,
                batch.actual_microdollars,
                None,
                None,
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        for mut item in items {
            if matches!(
                item.state,
                BatchItemState::Validated | BatchItemState::Activated
            ) {
                continue;
            }
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            let estimate = item.estimated_microdollars;
            batch = repo
                .reserve_music_generation_budget(batch_id, batch.revision, estimate, now_ms())
                .map_err(|e| e.to_string())?;
            repo.update_music_generation_item(
                &item.item_id,
                item.revision,
                BatchItemState::GeneratingAudio,
                None,
                (None, None, None),
                (None, None),
                (None, None),
                0,
                None,
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
            item = reload_item(&mut repo, batch_id, &item.item_id)?;
            let prompt_value: Value = serde_json::from_str(&item.prompt_json)
                .map_err(|_| "The saved prompt is invalid.".to_owned())?;
            let local_prompt = prompt_value
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("instrumental focus music")
                .to_owned();
            let mut text_cost = 0_u64;
            let final_prompt = if let Some(model) = &item.text_model {
                match client.refine_prompt(&key, model, &prompt_value) {
                    Ok(response) => {
                        record_attempt(
                            &mut repo,
                            &item.item_id,
                            AttemptKind::Text,
                            response.request_id.as_deref(),
                            estimate,
                            response.usage.cost_microdollars,
                            "succeeded",
                            None,
                        )?;
                        text_cost = response.usage.cost_microdollars;
                        repo.update_music_generation_item(
                            &item.item_id,
                            item.revision,
                            BatchItemState::GeneratingAudio,
                            Some(&response.text),
                            (
                                None,
                                Some(response.request_id.as_deref().unwrap_or("")),
                                None,
                            ),
                            (None, None),
                            (None, None),
                            response.usage.cost_microdollars,
                            None,
                            None,
                            now_ms(),
                        )
                        .map_err(|e| e.to_string())?;
                        item = reload_item(&mut repo, batch_id, &item.item_id)?;
                        response.text
                    }
                    Err(_) => {
                        record_attempt(
                            &mut repo,
                            &item.item_id,
                            AttemptKind::Text,
                            None,
                            estimate,
                            0,
                            "failed",
                            Some("text_refinement_failed"),
                        )?;
                        local_prompt
                    }
                }
            } else {
                local_prompt
            };
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            let audio = match generate_audio_with_retry(
                &client,
                &key,
                &item.audio_model,
                &final_prompt,
                request.duration_seconds,
            ) {
                Ok(response) => {
                    record_attempt(
                        &mut repo,
                        &item.item_id,
                        AttemptKind::Audio,
                        response.request_id.as_deref(),
                        estimate,
                        response.usage.cost_microdollars,
                        "succeeded",
                        None,
                    )?;
                    response
                }
                Err(error) => {
                    record_attempt(
                        &mut repo,
                        &item.item_id,
                        AttemptKind::Audio,
                        None,
                        estimate,
                        0,
                        "failed",
                        Some("audio_generation_failed"),
                    )?;
                    return Err(error.message);
                }
            };
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            let folder = self
                .root
                .join("cloud-generation")
                .join("batches")
                .join(batch_id);
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let audio_extension = detect_audio_extension(&audio.bytes).ok_or_else(|| {
                "The audio provider returned an unsupported container. Expected WAV, FLAC, or MP3."
                    .to_owned()
            })?;
            let audio_path = folder.join(format!("{}-audio.{audio_extension}", item.ordinal));
            write_regular(&audio_path, &audio.bytes)?;
            let audio_hash = sha256(&audio.bytes);
            let image_model = item.image_model.clone();
            let cover_path = if let Some(model) = image_model.as_deref() {
                repo.update_music_generation_item(
                    &item.item_id,
                    item.revision,
                    BatchItemState::GeneratingCover,
                    None,
                    (Some(audio.request_id.as_deref().unwrap_or("")), None, None),
                    (Some(audio_path.to_string_lossy().as_ref()), None),
                    (Some(&audio_hash), None),
                    audio.usage.cost_microdollars,
                    None,
                    None,
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
                item = reload_item(&mut repo, batch_id, &item.item_id)?;
                let cover_prompt = serde_json::from_str::<Value>(&item.prompt_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("cover_prompt")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| cover_prompt(&item.activity));
                match client.generate_cover(&key, model, &cover_prompt) {
                    Ok(cover) => {
                        record_attempt(
                            &mut repo,
                            &item.item_id,
                            AttemptKind::Image,
                            cover.request_id.as_deref(),
                            estimate,
                            cover.usage.cost_microdollars,
                            "succeeded",
                            None,
                        )?;
                        let ext = if cover.mime_type.contains("jpeg") {
                            "jpg"
                        } else {
                            "png"
                        };
                        let path = folder.join(format!("{}-cover.{}", item.ordinal, ext));
                        write_regular(&path, &cover.bytes)?;
                        Some((path, cover))
                    }
                    Err(_) => {
                        record_attempt(
                            &mut repo,
                            &item.item_id,
                            AttemptKind::Image,
                            None,
                            estimate,
                            0,
                            "failed",
                            Some("image_generation_failed"),
                        )?;
                        let path = folder.join(format!("{}-cover.svg", item.ordinal));
                        let bytes = local_cover(&item.activity).into_bytes();
                        write_regular(&path, &bytes)?;
                        Some((
                            path,
                            openrouter::MediaResponse {
                                request_id: None,
                                bytes,
                                mime_type: "image/svg+xml".into(),
                                usage: Default::default(),
                            },
                        ))
                    }
                }
            } else {
                None
            };
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            let report = validate_audio(&audio_path, request.duration_seconds);
            let Some((validation_json, _)) = report else {
                repo.update_music_generation_item(
                    &item.item_id,
                    item.revision,
                    BatchItemState::Failed,
                    None,
                    (Some(audio.request_id.as_deref().unwrap_or("")), None, None),
                    (Some(audio_path.to_string_lossy().as_ref()), None),
                    (Some(&audio_hash), None),
                    audio.usage.cost_microdollars,
                    None,
                    Some((
                        "output_invalid",
                        "The generated audio failed local validation.",
                    )),
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
                return Err("One generated track failed local validation.".into());
            };
            let cover_path_str = cover_path
                .as_ref()
                .map(|(path, _)| path.to_string_lossy().to_string());
            let cover_hash = cover_path.as_ref().and_then(|(_, cover)| {
                if cover.bytes.is_empty() {
                    None
                } else {
                    Some(sha256(&cover.bytes))
                }
            });
            let cover_cost = cover_path
                .as_ref()
                .map(|(_, c)| c.usage.cost_microdollars)
                .unwrap_or(0);
            let actual = if audio.usage.cost_microdollars > 0
                && cover_path.as_ref().is_none_or(|(_, _)| cover_cost > 0)
            {
                text_cost
                    .saturating_add(audio.usage.cost_microdollars)
                    .saturating_add(cover_cost)
            } else {
                // A missing provider usage object is not treated as free.
                // Charge the reserved estimate for accounting and keep the
                // batch fail-closed if the provider later reports a higher
                // amount.
                estimate
            };
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            let remaining_reserved = batch.reserved_microdollars.saturating_sub(estimate);
            if batch
                .actual_microdollars
                .saturating_add(actual)
                .saturating_add(remaining_reserved)
                > batch.budget_microdollars
            {
                repo.update_music_generation_item(
                    &item.item_id,
                    item.revision,
                    BatchItemState::Failed,
                    None,
                    (Some(audio.request_id.as_deref().unwrap_or("")), None, None),
                    (
                        Some(audio_path.to_string_lossy().as_ref()),
                        cover_path_str.as_deref(),
                    ),
                    (Some(&audio_hash), cover_hash.as_deref()),
                    actual,
                    Some(&validation_json),
                    Some((
                        "budget_exceeded",
                        "The provider cost would exceed the authorized generation budget.",
                    )),
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
                return Err("The cloud generation budget would be exceeded.".into());
            }
            repo.update_music_generation_item(
                &item.item_id,
                item.revision,
                BatchItemState::Validated,
                None,
                (Some(audio.request_id.as_deref().unwrap_or("")), None, None),
                (
                    Some(audio_path.to_string_lossy().as_ref()),
                    cover_path_str.as_deref(),
                ),
                (Some(&audio_hash), cover_hash.as_deref()),
                actual,
                Some(&validation_json),
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
            batch = repo
                .load_music_generation_batch(batch_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "The cloud batch disappeared.".to_owned())?;
            batch = repo
                .update_music_generation_batch(
                    batch_id,
                    batch.revision,
                    BatchState::Running,
                    batch.reserved_microdollars.saturating_sub(estimate),
                    batch.actual_microdollars.saturating_add(actual),
                    None,
                    None,
                    None,
                    now_ms(),
                )
                .map_err(|e| e.to_string())?;
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.mark_batch_cancelled(&mut repo, batch_id)?;
            return Ok(());
        }
        repo.update_music_generation_batch(
            batch_id,
            batch.revision,
            BatchState::Validated,
            batch.reserved_microdollars,
            batch.actual_microdollars,
            None,
            None,
            None,
            now_ms(),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(feature = "cloud-generation-mock")]
    fn run_mock_batch(
        &self,
        batch_id: &str,
        request: &CloudGenerationRequest,
    ) -> Result<(), String> {
        use std::time::Duration;

        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let mut batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        batch = repo
            .update_music_generation_batch(
                batch_id,
                batch.revision,
                BatchState::Running,
                batch.reserved_microdollars,
                batch.actual_microdollars,
                None,
                None,
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
        let fixture =
            include_bytes!("../../../../crates/audio-engine/tests/fixtures/mp3_stereo_48000.mp3");
        let items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        for mut item in items {
            if matches!(
                item.state,
                BatchItemState::Validated | BatchItemState::Activated
            ) {
                continue;
            }
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            repo.update_music_generation_item(
                &item.item_id,
                item.revision,
                BatchItemState::GeneratingAudio,
                None,
                (None, None, None),
                (None, None),
                (None, None),
                0,
                None,
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
            thread::sleep(Duration::from_millis(150));
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            item = reload_item(&mut repo, batch_id, &item.item_id)?;
            let folder = self
                .root
                .join("cloud-generation")
                .join("batches")
                .join(batch_id);
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let audio_path = folder.join(format!("{}-audio.mp3", item.ordinal));
            write_regular(&audio_path, fixture)?;
            let audio_hash = sha256(fixture);
            let (cover_path, cover_hash) = if item.image_model.is_some() {
                let path = folder.join(format!("{}-cover.svg", item.ordinal));
                let bytes = local_cover(&item.activity).into_bytes();
                write_regular(&path, &bytes)?;
                (
                    Some(path.to_string_lossy().to_string()),
                    Some(sha256(&bytes)),
                )
            } else {
                (None, None)
            };
            let metadata = analyze_cloud_audio(&audio_path)?;
            let validation_json = json!({
                "mock": true,
                "codec": metadata.codec,
                "duration_seconds": metadata.duration_seconds,
                "requested_duration_seconds": request.duration_seconds,
            })
            .to_string();
            if self.cancel.load(Ordering::SeqCst) {
                self.mark_batch_cancelled(&mut repo, batch_id)?;
                return Ok(());
            }
            repo.update_music_generation_item(
                &item.item_id,
                item.revision,
                BatchItemState::Validated,
                None,
                (Some("mock-audio"), None, None),
                (
                    Some(audio_path.to_string_lossy().as_ref()),
                    cover_path.as_deref(),
                ),
                (Some(&audio_hash), cover_hash.as_deref()),
                0,
                Some(&validation_json),
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
            batch = repo
                .load_music_generation_batch(batch_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "The cloud batch disappeared.".to_owned())?;
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.mark_batch_cancelled(&mut repo, batch_id)?;
            return Ok(());
        }
        repo.update_music_generation_batch(
            batch_id,
            batch.revision,
            BatchState::Validated,
            batch.reserved_microdollars,
            batch.actual_microdollars,
            None,
            None,
            None,
            now_ms(),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn activate_batch(&self, batch_id: &str) -> Result<CloudBatchSummary, String> {
        let mut repo = PreferencesRepository::open(&self.database).map_err(|e| e.to_string())?;
        let batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch could not be found.".to_owned())?;
        if matches!(batch.state, BatchState::Activated) {
            let items = repo
                .list_music_generation_items(batch_id)
                .map_err(|e| e.to_string())?;
            return Ok(summary_from(&batch, &items));
        }
        if !matches!(batch.state, BatchState::Validated) {
            return Err("Only a fully validated batch can be saved to the library.".into());
        }
        let generated_items = repo
            .list_music_generation_items(batch_id)
            .map_err(|e| e.to_string())?;
        if generated_items.len() != usize::from(batch.target_count)
            || generated_items
                .iter()
                .any(|item| !matches!(item.state, BatchItemState::Validated))
        {
            return Err(
                "The batch is not ready to save. Wait for every track to finish validation.".into(),
            );
        }
        let version = format!("cloud-{}", now_ms());
        let active = self.root.join("cloud-generation").join("active.json");
        let previous = if active.exists() {
            fs::read_to_string(&active)
                .ok()
                .and_then(|v| serde_json::from_str::<Value>(&v).ok())
                .and_then(|v| v.get("version").and_then(Value::as_str).map(str::to_owned))
        } else {
            None
        };
        fs::create_dir_all(active.parent().unwrap_or(&self.root)).map_err(|e| e.to_string())?;
        let mut active_items = Vec::with_capacity(generated_items.len());
        for item in &generated_items {
            let audio_path = item
                .audio_path
                .as_deref()
                .ok_or_else(|| "A validated cloud item has no audio output.".to_owned())?;
            let metadata = analyze_cloud_audio_strict(Path::new(audio_path))?;
            let title = format!(
                "{} soundscape {}",
                item.activity.replace('_', " "),
                item.ordinal.saturating_add(1)
            );
            let cover_mime_type = item.cover_path.as_deref().and_then(|path| {
                Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| match extension.to_ascii_lowercase().as_str() {
                        "jpg" | "jpeg" => "image/jpeg",
                        "svg" => "image/svg+xml",
                        _ => "image/png",
                    })
            });
            let cover_sha256 = item
                .cover_path
                .as_deref()
                .and_then(|path| fs::read(path).ok())
                .map(|bytes| sha256(&bytes));
            active_items.push(json!({
                "item_id": item.item_id,
                "title": title,
                "activity": item.activity,
                "audio_path": audio_path,
                "audio_codec": metadata.codec,
                "cover_path": item.cover_path,
                "cover_mime_type": cover_mime_type,
                "audio_sha256": item.audio_sha256,
                "cover_sha256": cover_sha256,
                "sample_rate_hz": metadata.sample_rate_hz,
                "channels": metadata.channels,
                "bit_depth": metadata.bit_depth,
                "duration_seconds": metadata.duration_seconds,
                "genre_id": Value::Null,
                "mood_id": Value::Null,
            }));
        }
        let payload = json!({
            "version":version,
            "batch_id":batch_id,
            "activated_at_ms":now_ms(),
            "previous_version":previous,
            "items":active_items,
        });
        if let Some(previous_version) = previous.as_deref() {
            let history = active.parent().unwrap_or(&self.root).join("history");
            fs::create_dir_all(&history).map_err(|e| e.to_string())?;
            let current_bytes = fs::read(&active).map_err(|e| e.to_string())?;
            fs::write(
                history.join(format!("{previous_version}.json")),
                current_bytes,
            )
            .map_err(|e| e.to_string())?;
        }
        let temp = active.with_extension("json.tmp");
        fs::write(
            &temp,
            serde_json::to_vec(&payload).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&temp, &active).map_err(|e| e.to_string())?;
        let batch = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch disappeared.".to_owned())?;
        let _ = repo
            .update_music_generation_batch(
                batch_id,
                batch.revision,
                BatchState::Activated,
                batch.reserved_microdollars,
                batch.actual_microdollars,
                Some(&version),
                previous.as_deref(),
                None,
                now_ms(),
            )
            .map_err(|e| e.to_string())?;
        let activated = repo
            .load_music_generation_batch(batch_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The cloud batch disappeared after activation.".to_owned())?;
        Ok(summary_from(&activated, &generated_items))
    }
}

fn generate_audio_with_retry(
    client: &openrouter::OpenRouterClient,
    key: &str,
    model: &str,
    prompt: &str,
    duration_seconds: u16,
) -> Result<openrouter::MediaResponse, openrouter::OpenRouterError> {
    let mut last_error = None;
    for attempt in 0..AUDIO_RETRY_ATTEMPTS {
        match client.generate_audio(key, model, prompt, duration_seconds) {
            Ok(response) => return Ok(response),
            Err(error) if error.retryable && attempt + 1 < AUDIO_RETRY_ATTEMPTS => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_secs(2_u64 << attempt));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("audio retry loop must retain its last error"))
}

impl Drop for CloudGenerationService {
    fn drop(&mut self) {
        // Provider calls cannot be force-cancelled, but the worker checks this
        // flag after every provider boundary and will not finalize media after
        // the application begins shutting down.
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn validate_request(request: &CloudGenerationRequest) -> Result<(), String> {
    if !(1..=100).contains(&request.target_count) {
        return Err("Choose between 1 and 100 tracks.".into());
    }
    if request.activities.is_empty() {
        return Err("Choose at least one activity.".into());
    }
    if !(30..=600).contains(&request.duration_seconds) {
        return Err("Duration must be between 30 seconds and 10 minutes.".into());
    }
    if request.audio_model.trim().is_empty() {
        return Err("Choose an audio model.".into());
    }
    if request.budget_microdollars == 0 && !cloud_mock_enabled() {
        return Err("Set a maximum budget before generating.".into());
    }
    if request.note.as_ref().is_some_and(|note| note.len() > 1000) {
        return Err("Additional guidance is too long.".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ModelCostKind {
    Audio,
    Text,
    Image,
}

fn model_cost(
    available: &[CloudModelDto],
    model_id: &str,
    kind: ModelCostKind,
) -> Result<u64, String> {
    let model = available
        .iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| format!("The selected model '{model_id}' is unavailable."))?;
    let pricing = &model.pricing;
    let cost = match kind {
        ModelCostKind::Audio => pricing.request.as_deref().or(pricing.image.as_deref()),
        ModelCostKind::Image => pricing
            .image_output
            .as_deref()
            .or(pricing.request.as_deref())
            .or(pricing.image.as_deref()),
        ModelCostKind::Text => {
            if let Some(request) = pricing.request.as_deref() {
                return parse_dollars(request, model_id);
            }
            let prompt = pricing.prompt.as_deref().map(parse_rate).transpose()?;
            let completion = pricing.completion.as_deref().map(parse_rate).transpose()?;
            return match (prompt, completion) {
                (Some(prompt), Some(completion)) => dollars_to_microdollars(
                    prompt * TEXT_INPUT_TOKENS_ESTIMATE as f64
                        + completion * TEXT_OUTPUT_TOKENS_ESTIMATE as f64,
                ),
                (Some(prompt), None) => {
                    dollars_to_microdollars(prompt * TEXT_INPUT_TOKENS_ESTIMATE as f64)
                }
                _ => Err(format!(
                    "OpenRouter did not provide pricing for selected model '{model_id}'."
                )),
            };
        }
    };
    let rate = cost.ok_or_else(|| {
        format!("OpenRouter did not provide pricing for selected model '{model_id}'.")
    })?;
    parse_dollars(rate, model_id)
}

fn published_media_pricing(
    model_id: &str,
    mut pricing: openrouter::ModelPricing,
) -> openrouter::ModelPricing {
    // OpenRouter currently exposes Lyria's token rates as zero in /models,
    // while its model pages publish the actual per-output rates. Keep the
    // fixed price in the same request-rate field used by the estimator so the
    // user sees a truthful minimum before authorizing a batch.
    let request_rate_is_missing_or_zero = pricing
        .request
        .as_deref()
        .is_none_or(|value| parse_rate(value).is_ok_and(|rate| rate == 0.0));
    if request_rate_is_missing_or_zero {
        pricing.request = match model_id {
            "google/lyria-3-pro-preview" => Some("0.08".into()),
            "google/lyria-3-clip-preview" => Some("0.04".into()),
            _ => None,
        };
    }
    let image_rate_is_missing_or_underestimated = pricing
        .image_output
        .as_deref()
        .is_none_or(|value| parse_rate(value).is_ok_and(|rate| rate < 0.04));
    if image_rate_is_missing_or_underestimated && model_id == DEFAULT_IMAGE_MODEL {
        // OpenRouter's image model catalogue can expose token pricing while
        // the actual image response is billed as a fixed media output. Use a
        // conservative per-image estimate so the user is not surprised by a
        // batch that exhausts its budget after covers are generated.
        pricing.image_output = Some("0.04".into());
    }
    pricing
}

fn parse_dollars(value: &str, model_id: &str) -> Result<u64, String> {
    let rate = parse_rate(value).map_err(|_| {
        format!("OpenRouter returned invalid pricing for selected model '{model_id}'.")
    })?;
    dollars_to_microdollars(rate)
}

fn parse_rate(value: &str) -> Result<f64, String> {
    let rate = value
        .trim()
        .parse::<f64>()
        .map_err(|_| "model pricing is not numeric".to_owned())?;
    if !rate.is_finite() || rate.is_sign_negative() {
        return Err("model pricing is invalid".into());
    }
    Ok(rate)
}

fn dollars_to_microdollars(dollars: f64) -> Result<u64, String> {
    if !dollars.is_finite() || dollars.is_sign_negative() {
        return Err("model pricing is invalid".into());
    }
    let micros = (dollars * 1_000_000.0).ceil();
    if micros > u64::MAX as f64 {
        return Err("model pricing is outside the supported range".into());
    }
    Ok(micros as u64)
}

fn normalized_activities(input: &[String]) -> Vec<&str> {
    let allowed = [
        "deep_work",
        "motivation",
        "creativity",
        "learning",
        "light_work",
    ];
    let result: Vec<&str> = input
        .iter()
        .filter_map(|value| allowed.iter().copied().find(|item| *item == value))
        .collect();
    if result.is_empty() {
        allowed.to_vec()
    } else {
        result
    }
}

fn prompt_profile(activity: &str, ordinal: u16) -> Value {
    let profiles = match activity {
        "motivation" => vec![
            json!({"profile_id":"motivation-cinematic-drive","genre":"Cinematic","subgenre":"Orchestral","moods":["Driving","Energizing","Strong","Uplifting"],"instruments":["Acoustic Piano","Orchestral Brass","Orchestral Percussion","Orchestral Strings","Organic Percussion"],"bpm":104,"brightness":0.34,"complexity":0.70}),
            json!({"profile_id":"motivation-warm-pulse","genre":"Electronic","subgenre":"Electronica","moods":["Driving","Inspiring","Optimistic","Upbeat"],"instruments":["Acoustic Piano","Arp Synth Bass","Organic Percussion","Processed Strings","Synth Pad"],"bpm":112,"brightness":0.38,"complexity":0.62}),
            json!({"profile_id":"motivation-heroic-motion","genre":"Cinematic","subgenre":"Classical","moods":["Epic","Hopeful","Inspiring","Strong"],"instruments":["Acoustic Piano","Orchestral Brass","Orchestral Percussion","Orchestral Strings","Textural Soundscape"],"bpm":96,"brightness":0.29,"complexity":0.58}),
            json!({"profile_id":"motivation-deep-road","genre":"Atmospheric","subgenre":"Downtempo","moods":["Driving","Ponderous","Strong","Uplifting"],"instruments":["Acoustic Piano","Organic Percussion","Orchestral Strings","Synth Bass","Textural Soundscape"],"bpm":88,"brightness":0.24,"complexity":0.48}),
        ],
        "creativity" => vec![
            json!({"profile_id":"creativity-organic-flow","genre":"Atmospheric","subgenre":"Ambient","moods":["Hopeful","Inspiring","Optimistic","Uplifting"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion","Textural Soundscape"],"bpm":86,"brightness":0.28,"complexity":0.56}),
            json!({"profile_id":"creativity-cinematic-bloom","genre":"Cinematic","subgenre":"Orchestral","moods":["Epic","Hopeful","Inspiring","Ponderous"],"instruments":["Acoustic Piano","Orchestral Brass","Orchestral Strings","Orchestral Winds","Textural Soundscape"],"bpm":92,"brightness":0.32,"complexity":0.64}),
            json!({"profile_id":"creativity-warm-electronica","genre":"Electronic","subgenre":"Electronica","moods":["Driving","Inspiring","Optimistic","Upbeat"],"instruments":["Acoustic Piano","Arp Synth Bass","Organic Percussion","Processed Strings","Synth Pad"],"bpm":100,"brightness":0.36,"complexity":0.60}),
            json!({"profile_id":"creativity-still-current","genre":"Atmospheric","subgenre":"Downtempo","moods":["Mysterious","Ponderous","Hopeful","Inspiring"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion","Textural Soundscape"],"bpm":82,"brightness":0.22,"complexity":0.44}),
        ],
        "learning" => vec![
            json!({"profile_id":"learning-clear-cinematic","genre":"Cinematic","subgenre":"Classical","moods":["Hopeful","Inspiring","Optimistic","Uplifting"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion","Textural Soundscape"],"bpm":84,"brightness":0.27,"complexity":0.46}),
            json!({"profile_id":"learning-deep-orchestral","genre":"Cinematic","subgenre":"Orchestral","moods":["Ponderous","Inspiring","Strong","Uplifting"],"instruments":["Acoustic Piano","Orchestral Brass","Orchestral Percussion","Orchestral Strings"],"bpm":90,"brightness":0.30,"complexity":0.55}),
            json!({"profile_id":"learning-focused-electronica","genre":"Electronic","subgenre":"Ambient","moods":["Driving","Optimistic","Upbeat","Uplifting"],"instruments":["Acoustic Piano","Arp Synth Bass","Organic Percussion","Processed Strings","Synth Pad"],"bpm":98,"brightness":0.34,"complexity":0.52}),
            json!({"profile_id":"learning-quiet-depth","genre":"Piano","subgenre":"Acoustic","moods":["Hopeful","Ponderous","Inspiring"],"instruments":["Acoustic Piano","Orchestral Strings","Textural Soundscape"],"bpm":78,"brightness":0.20,"complexity":0.36}),
        ],
        "light_work" => vec![
            json!({"profile_id":"light-work-warm-air","genre":"Atmospheric","subgenre":"Ambient","moods":["Optimistic","Hopeful","Uplifting"],"instruments":["Acoustic Piano","Organic Percussion","Orchestral Strings","Textural Soundscape"],"bpm":88,"brightness":0.30,"complexity":0.42}),
            json!({"profile_id":"light-work-soft-motion","genre":"Electronic","subgenre":"Downtempo","moods":["Driving","Inspiring","Optimistic","Upbeat"],"instruments":["Acoustic Piano","Arp Synth Bass","Organic Percussion","Synth Pad"],"bpm":96,"brightness":0.35,"complexity":0.48}),
            json!({"profile_id":"light-work-acoustic-focus","genre":"Piano","subgenre":"Classical","moods":["Hopeful","Inspiring","Uplifting"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion"],"bpm":82,"brightness":0.24,"complexity":0.38}),
            json!({"profile_id":"light-work-calm-drive","genre":"Cinematic","subgenre":"Ambient","moods":["Driving","Ponderous","Optimistic"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion","Textural Soundscape"],"bpm":92,"brightness":0.28,"complexity":0.50}),
        ],
        _ => vec![
            json!({"profile_id":"deep-work-cinematic-depth","genre":"Cinematic","subgenre":"Orchestral","moods":["Ponderous","Hopeful","Inspiring","Strong"],"instruments":["Acoustic Piano","Orchestral Brass","Orchestral Percussion","Orchestral Strings","Textural Soundscape"],"bpm":84,"brightness":0.24,"complexity":0.52}),
            json!({"profile_id":"deep-work-warm-ambient","genre":"Atmospheric","subgenre":"Ambient","moods":["Mysterious","Ponderous","Hopeful"],"instruments":["Acoustic Piano","Orchestral Strings","Organic Percussion","Textural Soundscape"],"bpm":78,"brightness":0.18,"complexity":0.38}),
            json!({"profile_id":"deep-work-steady-electronica","genre":"Electronic","subgenre":"Electronica","moods":["Driving","Inspiring","Strong","Uplifting"],"instruments":["Acoustic Piano","Arp Synth Bass","Organic Percussion","Processed Strings","Synth Pad"],"bpm":92,"brightness":0.28,"complexity":0.56}),
            json!({"profile_id":"deep-work-low-light-piano","genre":"Piano","subgenre":"Classical","moods":["Ponderous","Hopeful","Inspiring"],"instruments":["Acoustic Piano","Orchestral Strings","Textural Soundscape"],"bpm":72,"brightness":0.16,"complexity":0.32}),
        ],
    };
    profiles[usize::from(ordinal) % profiles.len()].clone()
}

fn build_prompt(activity: &str, ordinal: u16, request: &CloudGenerationRequest) -> Value {
    let label = activity.replace('_', " ");
    let note = request.note.clone().unwrap_or_default();
    let profile = prompt_profile(activity, ordinal);
    let genre = profile
        .get("genre")
        .and_then(Value::as_str)
        .unwrap_or("Cinematic");
    let subgenre = profile
        .get("subgenre")
        .and_then(Value::as_str)
        .unwrap_or("Ambient");
    let moods = profile
        .get("moods")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let instruments = profile
        .get("instruments")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let bpm = profile.get("bpm").and_then(Value::as_u64).unwrap_or(84);
    json!({
        "activity": activity,
        "profile": profile,
        "prompt": format!("Instrumental high-end {genre} {subgenre} focus music for {label}. Create a complete {duration} second track at approximately {bpm} BPM. Moods: {moods}. Lead with mature real acoustic and orchestral instruments: {instruments}. Warm deep harmony, controlled low-mid energy, clear melodic identity, immediate musical movement within ten seconds, evolving sections and harmonic or instrumental development every 20 to 30 seconds, a purposeful ending or seamless long-form continuation. {note}" , duration=request.duration_seconds),
        "locked_negative": "vocals, lyrics, speech, choral voices, processed vocals, jazz, swing, blues, funk, hip-hop, rock, piercing high frequencies, high-pitched bells, shrill flute, harsh cymbals, sub-bass rumble, short repetitive loop, childish sound, MIDI-like mock instruments, abrupt silence",
        "cover_prompt": cover_prompt(activity),
        "duration_seconds": request.duration_seconds,
        "intro_max_seconds": 10,
        "variation_target_seconds": [20, 30]
    })
}

fn cover_prompt(activity: &str) -> String {
    format!(
        "Original Aria Focus cover art for {} focus music; abstract mature high-end visual, warm deep cinematic palette, strong sense of forward movement, no text, no logo, no people, no instruments, no gradients that band, no childish illustration",
        activity.replace('_', " ")
    )
}

fn summary_from(
    batch: &MusicGenerationBatch,
    items: &[MusicGenerationBatchItem],
) -> CloudBatchSummary {
    CloudBatchSummary {
        batch_id: batch.batch_id.clone(),
        state: batch.state.as_str().into(),
        target_count: batch.target_count,
        completed_count: items
            .iter()
            .filter(|i| {
                matches!(
                    i.state,
                    BatchItemState::Validated | BatchItemState::Activated
                )
            })
            .count() as u16,
        failed_count: items
            .iter()
            .filter(|i| matches!(i.state, BatchItemState::Failed))
            .count() as u16,
        reserved_microdollars: batch.reserved_microdollars,
        actual_microdollars: batch.actual_microdollars,
        budget_microdollars: batch.budget_microdollars,
        activation_version: batch.activation_version.clone(),
        error_code: batch.error_code.clone(),
        error_message: batch.error_message.clone(),
    }
}
fn item_dto(item: MusicGenerationBatchItem, cover_art: Option<String>) -> CloudBatchItemDto {
    CloudBatchItemDto {
        item_id: item.item_id,
        ordinal: item.ordinal,
        activity: item.activity,
        state: item.state.as_str().into(),
        audio_path: item.audio_path,
        cover_path: item.cover_path,
        cover_art,
        prompt_json: item.prompt_json,
        refined_prompt: item.refined_prompt,
        audio_sha256: item.audio_sha256,
        estimated_microdollars: item.estimated_microdollars,
        actual_microdollars: item.actual_microdollars,
        validation_json: item.validation_json,
        error_code: item.error_code,
        error_message: item.error_message,
    }
}

fn safe_batch_id(batch_id: &str) -> bool {
    batch_id.starts_with("cloud_batch_")
        && batch_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn reload_item(
    repo: &mut PreferencesRepository,
    batch_id: &str,
    item_id: &str,
) -> Result<MusicGenerationBatchItem, String> {
    repo.list_music_generation_items(batch_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|item| item.item_id == item_id)
        .ok_or_else(|| "The cloud batch item disappeared.".to_owned())
}

#[allow(clippy::too_many_arguments)]
fn record_attempt(
    repo: &mut PreferencesRepository,
    item_id: &str,
    kind: AttemptKind,
    request_id: Option<&str>,
    estimated_microdollars: u64,
    actual_microdollars: u64,
    state: &str,
    error_code: Option<&str>,
) -> Result<(), String> {
    let created_at_ms = now_ms();
    let attempt_id = format!("attempt_{}_{}_{}", item_id, kind.as_str(), created_at_ms);
    repo.record_music_generation_attempt(&MusicGenerationAttempt {
        attempt_id,
        item_id: item_id.to_owned(),
        kind,
        request_id: request_id.map(str::to_owned),
        estimated_microdollars,
        actual_microdollars,
        state: state.to_owned(),
        error_code: error_code.map(str::to_owned),
        created_at_ms,
        updated_at_ms: created_at_ms,
    })
    .map_err(|error| error.to_string())
}

fn unique_suffix() -> String {
    format!("{}", now_ms())
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn write_regular(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err("The output path already exists.".into());
    }
    fs::write(path, bytes).map_err(|e| e.to_string())
}
fn sha256(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}
fn local_cover(activity: &str) -> String {
    format!("<svg xmlns='http://www.w3.org/2000/svg' width='1024' height='1024'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop stop-color='#16132a'/><stop offset='1' stop-color='#8b5cf6'/></linearGradient></defs><rect width='1024' height='1024' fill='url(#g)'/><circle cx='512' cy='512' r='220' fill='none' stroke='#fff' stroke-opacity='.65' stroke-width='4'/><text x='512' y='540' text-anchor='middle' fill='white' font-family='sans-serif' font-size='56'>{}</text></svg>",activity.replace('_'," "))
}
fn validate_audio(path: &Path, expected_duration: u16) -> Option<(String, Value)> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() == 0 {
        return None;
    }
    let report = analyze_file(path);
    // Keep analyzer flags in the saved validation report so a provider
    // candidate can be previewed and reviewed. Hard flags are still enforced
    // when a track is activated or exported as release content.
    if report.decode.status != DecodeStatus::Decoded {
        return None;
    }
    if report.decode.duration_seconds.is_some_and(|duration| {
        let tolerance = PROVIDER_DURATION_TOLERANCE_SECONDS
            .max(f64::from(expected_duration) * PROVIDER_DURATION_TOLERANCE_RATIO);
        (duration - f64::from(expected_duration)).abs() > tolerance
    }) {
        return None;
    }
    let json = serde_json::to_value(&report).ok()?;
    Some((json.to_string(), json))
}

fn detect_audio_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some("wav");
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"fLaC" {
        return Some("flac");
    }
    if bytes.len() >= 3 && &bytes[0..3] == b"ID3" {
        return Some("mp3");
    }
    if bytes.len() >= 2 && bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0 {
        return Some("mp3");
    }
    None
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CloudAudioMetadata {
    pub(crate) codec: &'static str,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: u16,
    pub(crate) bit_depth: Option<u16>,
    pub(crate) duration_seconds: f32,
}

pub(crate) fn analyze_cloud_audio(path: &Path) -> Result<CloudAudioMetadata, String> {
    let report = analyze_file(path);
    if report.decode.status != DecodeStatus::Decoded {
        return Err("The generated audio could not be decoded for preview.".into());
    }
    cloud_audio_metadata(report.decode)
}

fn analyze_cloud_audio_strict(path: &Path) -> Result<CloudAudioMetadata, String> {
    let report = analyze_file(path);
    if report.decode.status != DecodeStatus::Decoded || report.has_hard_rejections() {
        return Err(
            "The generated audio failed local validation. Review or repair the technical flags before activation."
                .into(),
        );
    }
    cloud_audio_metadata(report.decode)
}

fn cloud_audio_metadata(
    decode: audio_analyzer::DecodeReport,
) -> Result<CloudAudioMetadata, String> {
    let codec = match decode.codec {
        Some(Codec::Wav) => "wav",
        Some(Codec::Flac) => "flac",
        Some(Codec::Mp3) => "mp3",
        None => return Err("The generated audio has no recognized codec.".into()),
    };
    Ok(CloudAudioMetadata {
        codec,
        sample_rate_hz: decode
            .sample_rate_hz
            .ok_or_else(|| "Generated audio has no sample rate.".to_owned())?,
        channels: decode
            .channels
            .ok_or_else(|| "Generated audio has no channel count.".to_owned())?,
        bit_depth: decode.bit_depth,
        duration_seconds: decode
            .duration_seconds
            .ok_or_else(|| "Generated audio has no duration.".to_owned())?
            as f32,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct WavMetadata {
    sample_rate_hz: u32,
    channels: u16,
    bit_depth: Option<u16>,
    duration_seconds: f32,
}

#[cfg(test)]
fn read_wav_metadata(path: &Path) -> Option<WavMetadata> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data_bytes = None;
    while offset.checked_add(8)? <= bytes.len() {
        let chunk = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let start = offset.checked_add(8)?;
        let end = start.checked_add(length)?.min(bytes.len());
        if end < start {
            return None;
        }
        if chunk == b"fmt " && length >= 16 && end >= start + 16 {
            let format_tag = u16::from_le_bytes(bytes[start..start + 2].try_into().ok()?);
            let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().ok()?);
            let sample_rate_hz = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().ok()?);
            let bit_depth = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().ok()?);
            if !matches!(format_tag, 1 | 3)
                || !matches!(channels, 1 | 2)
                || sample_rate_hz == 0
                || !matches!(bit_depth, 16 | 24 | 32)
            {
                return None;
            }
            format = Some((sample_rate_hz, channels, bit_depth));
        } else if chunk == b"data" {
            data_bytes = Some(length.min(bytes.len().saturating_sub(start)));
        }
        offset = end.saturating_add(length % 2);
    }
    let (sample_rate_hz, channels, bit_depth) = format?;
    let data_bytes = data_bytes?;
    let bytes_per_second = u64::from(sample_rate_hz)
        .checked_mul(u64::from(channels))?
        .checked_mul(u64::from(bit_depth / 8))?;
    if bytes_per_second == 0 || data_bytes == 0 {
        return None;
    }
    Some(WavMetadata {
        sample_rate_hz,
        channels,
        bit_depth: Some(bit_depth),
        duration_seconds: data_bytes as f32 / bytes_per_second as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(target_count: u16) -> CloudGenerationRequest {
        CloudGenerationRequest {
            target_count,
            activities: vec!["deep_work".into()],
            audio_model: "audio/test".into(),
            text_model: Some("text/test".into()),
            image_model: Some("image/test".into()),
            refine_prompts: true,
            generate_covers: true,
            duration_seconds: 180,
            budget_microdollars: 1_000_000,
            note: None,
        }
    }

    #[test]
    fn estimate_uses_selected_openrouter_model_pricing_and_scales_by_track_count() {
        let models = vec![
            CloudModelDto {
                id: "audio/test".into(),
                name: None,
                description: None,
                input_modalities: vec![],
                output_modalities: vec!["audio".into()],
                supported_parameters: vec![],
                pricing: openrouter::ModelPricing {
                    request: Some("0.08".into()),
                    ..Default::default()
                },
                context_length: None,
                curated: false,
            },
            CloudModelDto {
                id: "text/test".into(),
                name: None,
                description: None,
                input_modalities: vec![],
                output_modalities: vec!["text".into()],
                supported_parameters: vec![],
                pricing: openrouter::ModelPricing {
                    prompt: Some("0.000001".into()),
                    completion: Some("0.000002".into()),
                    ..Default::default()
                },
                context_length: None,
                curated: false,
            },
            CloudModelDto {
                id: "image/test".into(),
                name: None,
                description: None,
                input_modalities: vec![],
                output_modalities: vec!["image".into()],
                supported_parameters: vec![],
                pricing: openrouter::ModelPricing {
                    image: Some("0.03".into()),
                    ..Default::default()
                },
                context_length: None,
                curated: false,
            },
        ];
        let estimate =
            CloudGenerationService::estimate_with_models(&sample_request(5), &models).unwrap();
        assert_eq!(estimate.audio_microdollars, 400_000);
        assert_eq!(estimate.text_microdollars, 14_000);
        assert_eq!(estimate.image_microdollars, 150_000);
        assert_eq!(estimate.total_microdollars, 564_000);
        assert_eq!(
            estimate.pricing_source,
            "OpenRouter model pricing + published media rates"
        );
    }

    #[test]
    fn provider_mp3_container_is_detected_and_analyzed_without_wav_assumptions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("provider-output.mp3");
        let bytes =
            include_bytes!("../../../../crates/audio-engine/tests/fixtures/mp3_stereo_48000.mp3");
        fs::write(&path, bytes).unwrap();
        assert_eq!(detect_audio_extension(bytes), Some("mp3"));
        let metadata = analyze_cloud_audio(&path).unwrap();
        assert_eq!(metadata.codec, "mp3");
        assert_eq!(metadata.sample_rate_hz, 48_000);
        assert_eq!(metadata.channels, 2);
    }

    #[test]
    fn published_lyria_rates_fill_the_fixed_media_price_missing_from_model_catalog() {
        let pro = published_media_pricing("google/lyria-3-pro-preview", Default::default());
        let clip = published_media_pricing("google/lyria-3-clip-preview", Default::default());
        assert_eq!(pro.request.as_deref(), Some("0.08"));
        assert_eq!(clip.request.as_deref(), Some("0.04"));

        let zero_pricing = openrouter::ModelPricing {
            request: Some("0".into()),
            ..Default::default()
        };
        let pro_with_zero = published_media_pricing("google/lyria-3-pro-preview", zero_pricing);
        assert_eq!(pro_with_zero.request.as_deref(), Some("0.08"));

        let image_with_zero = published_media_pricing(
            DEFAULT_IMAGE_MODEL,
            openrouter::ModelPricing {
                image_output: Some("0".into()),
                ..Default::default()
            },
        );
        assert_eq!(image_with_zero.image_output.as_deref(), Some("0.04"));

        let image_with_catalogue_token_rate = published_media_pricing(
            DEFAULT_IMAGE_MODEL,
            openrouter::ModelPricing {
                image_output: Some("0.00003".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            image_with_catalogue_token_rate.image_output.as_deref(),
            Some("0.04")
        );
    }

    #[cfg(feature = "cloud-generation-mock")]
    #[test]
    fn mock_batch_generates_local_media_without_provider_usage() {
        let temp = tempfile::tempdir().unwrap();
        let service = CloudGenerationService::new(
            temp.path().join("preferences.sqlite3"),
            temp.path().join("content"),
        );
        let request = CloudGenerationRequest {
            target_count: 2,
            activities: vec!["motivation".into()],
            audio_model: "mock/audio".into(),
            text_model: Some("mock/text".into()),
            image_model: Some("mock/image".into()),
            refine_prompts: true,
            generate_covers: true,
            duration_seconds: 180,
            budget_microdollars: 0,
            note: None,
        };

        assert!(service.key_status().unwrap().mock);
        assert_eq!(service.estimate(&request).unwrap().total_microdollars, 0);
        let created = service.create_batch(request).unwrap();
        assert_eq!(created.state, "quoted");

        let completed = (0..50).find_map(|_| {
            let batch = service.get_batch(&created.batch_id).unwrap();
            if batch
                .as_ref()
                .is_some_and(|batch| batch.state == "validated")
            {
                batch
            } else {
                std::thread::sleep(std::time::Duration::from_millis(50));
                None
            }
        });
        let completed = completed.expect("mock batch should finish locally");
        assert_eq!(completed.completed_count, 2);
        assert_eq!(completed.actual_microdollars, 0);

        let items = service.get_items(&created.batch_id).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item.state == "validated"
                && item.actual_microdollars == 0
                && item.audio_path.is_some()
                && item.cover_path.is_some()
        }));
    }

    #[test]
    fn wav_metadata_is_read_from_the_container_not_the_requested_duration() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.wav");
        let bytes = include_bytes!(
            "../../../../crates/audio-engine/tests/fixtures/wav_pcm16_mono_44100.wav"
        );
        fs::write(&path, bytes).unwrap();
        let metadata = read_wav_metadata(&path).unwrap();
        assert_eq!(metadata.sample_rate_hz, 44_100);
        assert_eq!(metadata.channels, 1);
        assert_eq!(metadata.bit_depth, Some(16));
        assert!(metadata.duration_seconds > 0.0);
    }

    #[test]
    fn active_cloud_library_is_bounded_and_integrity_checked_when_media_is_used() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("content");
        let media = root.join("cloud-generation").join("batches");
        fs::create_dir_all(&media).unwrap();
        let audio_path = media.join("track.wav");
        let bytes = include_bytes!(
            "../../../../crates/audio-engine/tests/fixtures/wav_pcm16_mono_44100.wav"
        );
        fs::write(&audio_path, bytes).unwrap();
        let metadata = read_wav_metadata(&audio_path).unwrap();
        let payload = json!({
            "version":"cloud-test",
            "batch_id":"cloud_batch_test",
            "activated_at_ms":1,
            "previous_version":null,
            "items":[{
                "item_id":"cloud-item-test",
                "title":"Test soundscape",
                "activity":"deep_work",
                "audio_path":audio_path,
                "cover_path":null,
                "cover_mime_type":null,
                "cover_sha256":null,
                "audio_sha256":sha256(bytes),
                "sample_rate_hz":metadata.sample_rate_hz,
                "channels":metadata.channels,
                "bit_depth":metadata.bit_depth,
                "duration_seconds":metadata.duration_seconds,
                "genre_id":null,
                "mood_id":null
            }]
        });
        fs::write(
            root.join("cloud-generation").join("active.json"),
            serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();
        let library = load_active_library(&root).unwrap().unwrap();
        assert_eq!(library.items.len(), 1);
        assert_eq!(library.items[0].item_id, "cloud-item-test");

        let mut corrupt = payload;
        corrupt["items"][0]["audio_sha256"] = json!("0".repeat(64));
        fs::write(
            root.join("cloud-generation").join("active.json"),
            serde_json::to_vec(&corrupt).unwrap(),
        )
        .unwrap();
        // The activation record remains structurally readable; the decoder
        // performs the final hash check for the selected track.
        assert!(load_active_library(&root).unwrap().is_some());
    }
}
