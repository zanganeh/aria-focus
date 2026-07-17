use std::collections::{HashMap, HashSet};

use domain::Activity;
use semver::{Prerelease, Version, VersionReq};
use serde::{Deserialize, Serialize};

pub const MANIFEST_PATH: &str = "manifest.json";
pub const FORMAT_NAME: &str = "adhdpack";
/// App-owned Studio output and the original lossless/import format remain v1.
pub const FORMAT_VERSION: u32 = 1;
pub const FORMAT_VERSION_2: u32 = 2;

/// The only trust mode for files created by this application's local Studio.
/// It is intentionally a record, rather than an archive format: callers must
/// supply it from the controlled generation staging area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedLocalRecord {
    pub generation_job_id: String,
    pub manifest: ContentPackManifest,
    pub evidence: LocalGenerationEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGenerationEvidence {
    pub producer: String,
    pub job_id: String,
    pub completed_at_unix_seconds: i64,
}

/// Calibration policy values. These are conservative ingest gates, not
/// scientific or clinical thresholds.
pub mod calibration_policy {
    pub const MAX_VOCAL_SPEECH_LIKELIHOOD: f32 = 0.05;
    pub const MAX_TRACK_SECONDS: f32 = 8.0 * 60.0 * 60.0;
    pub const MIN_TRACK_SECONDS: f32 = 1.0;
    pub const MIN_INTEGRATED_LUFS: f32 = -80.0;
    pub const MAX_INTEGRATED_LUFS: f32 = 0.0;
    pub const MIN_TRUE_PEAK_DBFS: f32 = -120.0;
    pub const MAX_TRUE_PEAK_DBFS: f32 = 0.0;
    pub const MAX_LOUDNESS_RANGE_LU: f32 = 60.0;
    pub const MAX_SPECTRAL_CENTROID_HZ: f32 = 48_000.0;
    pub const MAX_ONSET_DENSITY_PER_SECOND: f32 = 50.0;
    pub const MIN_TEMPO_BPM: f32 = 20.0;
    pub const MAX_TEMPO_BPM: f32 = 300.0;
    pub const MAX_TEMPO_DRIFT_PERCENT: f32 = 20.0;
    pub const MIN_HUMAN_REVIEWS: usize = 2;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentPackManifest {
    pub format: String,
    pub format_version: u32,
    pub pack: PackMetadata,
    pub taxonomy: Taxonomy,
    pub items: Vec<ContentItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackMetadata {
    pub id: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub app_version_requirement: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taxonomy {
    pub genres: Vec<TaxonomyTerm>,
    pub moods: Vec<TaxonomyTerm>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyTerm {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentItem {
    pub id: String,
    pub title: String,
    pub genre_ids: Vec<String>,
    pub mood_ids: Vec<String>,
    pub activity_suitability: Vec<ActivitySuitability>,
    pub provenance: Provenance,
    pub analysis: TechnicalAnalysis,
    pub variants: Vec<ContentVariant>,
    pub human_qa: HumanQa,
    /// Optional declared cover-art asset for the item. Older manifests omit
    /// this field; `#[serde(default)]` keeps them valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<CoverArtAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivitySuitability {
    pub activity: Activity,
    pub suitability: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub licence_id: String,
    pub licence_url: Option<String>,
    pub composer: Option<String>,
    pub generator: Option<GeneratorMetadata>,
    pub contains_lyrics: bool,
    pub contains_speech: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorMetadata {
    pub provider: String,
    pub model: String,
    pub model_version: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalAnalysis {
    pub duration_seconds: f32,
    pub integrated_lufs: f32,
    pub true_peak_dbfs: f32,
    pub loudness_range_lu: f32,
    pub spectral_centroid_hz: f32,
    pub high_frequency_energy_ratio: f32,
    pub onset_density_per_second: f32,
    pub tempo_bpm: f32,
    pub tempo_confidence: f32,
    pub tempo_drift_percent: f32,
    pub section_change_novelty: f32,
    pub unexplained_silence_seconds: f32,
    pub clipped_samples: u64,
    pub discontinuity_detected: bool,
    pub codec_errors_detected: bool,
    pub corruption_detected: bool,
    pub vocal_speech_likelihood: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentVariant {
    pub id: String,
    pub asset: AudioAsset,
    pub safe_regions: Vec<SafeRegion>,
    pub stimulation_available: Vec<StimulationAvailability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioAsset {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub codec: AssetCodec,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bit_depth: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetCodec {
    Wav,
    Flac,
    Mp3,
    OggOpus,
}

impl AssetCodec {
    pub fn expected_extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::OggOpus => "opus",
        }
    }
}

/// Declared cover-art asset for a content item. The renderer only ever sees a
/// bounded data URL derived from this validated, installed asset; the path
/// itself never leaves the catalogue layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverArtAsset {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: CoverArtFormat,
    pub width: u32,
    pub height: u32,
    /// Art provenance recorded with the binary asset so the replacement
    /// manifest attributes each cover to its generator or licence.
    pub provenance: CoverArtProvenance,
}

/// Provenance for a cover-art asset. A generated cover supplies `generator`
/// (provider/model/model_version/prompt); a licensed cover supplies a
/// `licence_id`. At least one of the two must be declared so the lineage is
/// never anonymous.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverArtProvenance {
    pub source: String,
    pub generator: Option<GeneratorMetadata>,
    pub licence_id: Option<String>,
    pub licence_url: Option<String>,
}

/// Supported raster cover-art formats. The MIME type is derived from the
/// format, so extension/MIME consistency is enforced structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverArtFormat {
    Png,
    Webp,
    Jpeg,
}

impl CoverArtFormat {
    pub fn expected_extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Jpeg => "jpg",
        }
    }

    pub fn expected_mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Jpeg => "image/jpeg",
        }
    }
}

/// A declared asset enumerated for installed-pack verification. It owns no
/// new data: callers borrow either an audio asset or a cover-art asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeclaredAsset<'a> {
    Audio(&'a AudioAsset),
    Cover(&'a CoverArtAsset),
}

impl DeclaredAsset<'_> {
    pub fn path(&self) -> &str {
        match self {
            DeclaredAsset::Audio(asset) => &asset.path,
            DeclaredAsset::Cover(asset) => &asset.path,
        }
    }

    pub fn sha256(&self) -> &str {
        match self {
            DeclaredAsset::Audio(asset) => &asset.sha256,
            DeclaredAsset::Cover(asset) => &asset.sha256,
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            DeclaredAsset::Audio(asset) => asset.bytes,
            DeclaredAsset::Cover(asset) => asset.bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafeRegion {
    pub kind: SafeRegionKind,
    pub start_seconds: f32,
    pub end_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeRegionKind {
    Loop,
    Crossfade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StimulationAvailability {
    Off,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanQa {
    pub status: HumanQaStatus,
    pub reviews: Vec<HumanReview>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanQaStatus {
    Draft,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanReview {
    pub reviewer_id: String,
    pub reviewed_at: String,
    pub notes: String,
    pub representative_work_session: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("manifest validation failed: {issues}", issues = .0.join("; "))]
pub struct ManifestValidationError(pub Vec<String>);

impl ContentPackManifest {
    /// Validates the narrow local-generation contract. This does not assert
    /// vocal or speech detection; Studio output has no such verification.
    pub fn validate_generated_local(
        &self,
        generation_job_id: &str,
    ) -> Result<(), ManifestValidationError> {
        let mut issues = Vec::new();
        if !is_stable_identifier(generation_job_id) {
            issues.push("generation_job_id must be a stable identifier".to_owned());
        }
        if self.format != FORMAT_NAME || self.format_version != FORMAT_VERSION {
            issues.push("generated-local record has an unsupported format".to_owned());
        }
        if self.pack.version != "1.0.0" || self.pack.app_version_requirement != "*" {
            issues.push("generated-local version metadata is not backend-owned".to_owned());
        }
        if self.pack.title.trim().is_empty() || self.pack.title.len() > 120 {
            issues.push("generated-local title is invalid".to_owned());
        }
        if self.items.len() != 1 {
            issues.push("generated-local record must contain exactly one item".to_owned());
        }
        let expected_pack = format!("generated.local.{generation_job_id}");
        if self.pack.id != expected_pack {
            issues.push("generated-local pack id is not backend-owned".to_owned());
        }
        if let Some(item) = self.items.first() {
            let expected_item = format!("generated.local.{generation_job_id}.item");
            if item.id != expected_item {
                issues.push("generated-local item id is not backend-owned".to_owned());
            }
            if item.human_qa.status != HumanQaStatus::Draft || !item.human_qa.reviews.is_empty() {
                issues.push(
                    "generated-local record may not claim human or vocal/speech verification"
                        .to_owned(),
                );
            }
            if item.cover.is_some() {
                issues.push("generated-local record may not declare cover art".to_owned());
            }
            if item.variants.len() != 1 {
                issues.push(
                    "generated-local record must contain exactly one FLAC variant".to_owned(),
                );
            }
            for variant in &item.variants {
                let expected_path = format!("assets/generated/{generation_job_id}.flac");
                if variant.asset.path != expected_path {
                    issues.push("generated-local asset path is not backend-owned".to_owned());
                }
                if variant.asset.codec != AssetCodec::Flac
                    || variant.asset.sample_rate_hz != 48_000
                    || variant.asset.channels != 2
                {
                    issues.push("generated-local audio must be 48 kHz stereo FLAC".to_owned());
                }
                if variant.asset.bytes == 0
                    || variant.asset.sha256.len() != 64
                    || !variant.asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    issues.push("generated-local audio must declare bytes and SHA-256".to_owned());
                }
                if !item.analysis.duration_seconds.is_finite()
                    || !(calibration_policy::MIN_TRACK_SECONDS
                        ..=calibration_policy::MAX_TRACK_SECONDS)
                        .contains(&item.analysis.duration_seconds)
                {
                    issues.push("generated-local audio must declare a valid duration".to_owned());
                }
            }
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestValidationError(issues))
        }
    }
    pub fn validate_published(&self) -> Result<(), ManifestValidationError> {
        let mut issues = Vec::new();
        if self.format != FORMAT_NAME {
            issues.push(format!("format must be {FORMAT_NAME}"));
        }
        if !matches!(self.format_version, FORMAT_VERSION | FORMAT_VERSION_2) {
            issues.push(format!(
                "unsupported format_version {}",
                self.format_version
            ));
        }
        validate_id("pack.id", &self.pack.id, &mut issues);
        required("pack.title", &self.pack.title, &mut issues);
        required("pack.description", &self.pack.description, &mut issues);
        if Version::parse(&self.pack.version).is_err() {
            issues.push("pack.version must be valid SemVer".to_owned());
        }
        if VersionReq::parse(&self.pack.app_version_requirement).is_err() {
            issues
                .push("pack.app_version_requirement must be a valid SemVer requirement".to_owned());
        }

        let genre_ids = validate_taxonomy("genre", &self.taxonomy.genres, &mut issues);
        let mood_ids = validate_taxonomy("mood", &self.taxonomy.moods, &mut issues);
        if self.items.is_empty() {
            issues.push("pack must contain at least one item".to_owned());
        }
        let mut item_ids = HashSet::new();
        let mut asset_paths = HashSet::new();
        for item in &self.items {
            validate_item(
                item,
                self.format_version,
                &genre_ids,
                &mood_ids,
                &mut item_ids,
                &mut asset_paths,
                &mut issues,
            );
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestValidationError(issues))
        }
    }

    /// Validates the non-human-QA contract for an owner-waived pack that is
    /// trusted separately by the application build. This is deliberately not
    /// used by archive import or ordinary installed-pack revalidation.
    pub fn validate_bundled_owner_waived(&self) -> Result<(), ManifestValidationError> {
        let mut issues = Vec::new();
        if self.format != FORMAT_NAME {
            issues.push(format!("format must be {FORMAT_NAME}"));
        }
        if !matches!(self.format_version, FORMAT_VERSION | FORMAT_VERSION_2) {
            issues.push(format!(
                "unsupported format_version {}",
                self.format_version
            ));
        }
        validate_id("pack.id", &self.pack.id, &mut issues);
        required("pack.title", &self.pack.title, &mut issues);
        required("pack.description", &self.pack.description, &mut issues);
        if Version::parse(&self.pack.version).is_err() {
            issues.push("pack.version must be valid SemVer".to_owned());
        }
        if VersionReq::parse(&self.pack.app_version_requirement).is_err() {
            issues
                .push("pack.app_version_requirement must be a valid SemVer requirement".to_owned());
        }
        let genre_ids = validate_taxonomy("genre", &self.taxonomy.genres, &mut issues);
        let mood_ids = validate_taxonomy("mood", &self.taxonomy.moods, &mut issues);
        if self.items.is_empty() {
            issues.push("pack must contain at least one item".to_owned());
        }
        let mut item_ids = HashSet::new();
        let mut asset_paths = HashSet::new();
        for item in &self.items {
            validate_item_without_human_qa(
                item,
                self.format_version,
                &genre_ids,
                &mood_ids,
                &mut item_ids,
                &mut asset_paths,
                &mut issues,
            );
            if item.human_qa.status != HumanQaStatus::Draft || !item.human_qa.reviews.is_empty() {
                issues.push(format!(
                    "item {} must remain draft with no claimed human reviews for owner-waived private beta",
                    item.id
                ));
            }
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestValidationError(issues))
        }
    }

    pub fn validate_app_compatibility(
        &self,
        app_version: &str,
    ) -> Result<(), ManifestValidationError> {
        let mut version = Version::parse(app_version).map_err(|_| {
            ManifestValidationError(vec!["app version is not valid SemVer".to_owned()])
        })?;
        let requirement = VersionReq::parse(&self.pack.app_version_requirement).map_err(|_| {
            ManifestValidationError(vec![
                "pack.app_version_requirement is not valid SemVer".to_owned()
            ])
        })?;
        // Content compatibility follows the app's API release line. SemVer
        // requirements intentionally exclude prereleases by default, which
        // would otherwise make ordinary ranges (and even `*`) reject every
        // pack while the desktop app is in public beta.
        version.pre = Prerelease::EMPTY;
        if requirement.matches(&version) {
            Ok(())
        } else {
            Err(ManifestValidationError(vec![format!(
                "pack requires app version {}, but this app is {app_version}",
                self.pack.app_version_requirement
            )]))
        }
    }

    pub fn canonicalized(&self) -> Self {
        let mut manifest = self.clone();
        manifest.taxonomy.genres.sort_by(|a, b| a.id.cmp(&b.id));
        manifest.taxonomy.moods.sort_by(|a, b| a.id.cmp(&b.id));
        manifest.items.sort_by(|a, b| a.id.cmp(&b.id));
        for item in &mut manifest.items {
            item.genre_ids.sort();
            item.mood_ids.sort();
            item.activity_suitability
                .sort_by_key(|entry| entry.activity.storage_key());
            item.variants.sort_by(|a, b| a.id.cmp(&b.id));
            item.human_qa
                .reviews
                .sort_by(|a, b| a.reviewer_id.cmp(&b.reviewer_id));
            for variant in &mut item.variants {
                variant.safe_regions.sort_by(|a, b| {
                    a.start_seconds
                        .total_cmp(&b.start_seconds)
                        .then(a.end_seconds.total_cmp(&b.end_seconds))
                });
                variant
                    .stimulation_available
                    .sort_by_key(|level| stimulation_order(*level));
            }
        }
        manifest
    }

    pub fn declared_assets(&self) -> HashMap<&str, &AudioAsset> {
        self.items
            .iter()
            .flat_map(|item| item.variants.iter().map(|variant| &variant.asset))
            .map(|asset| (asset.path.as_str(), asset))
            .collect()
    }

    /// Declared cover-art assets keyed by their canonical path. Items without
    /// cover art contribute nothing, so older manifests enumerate empty.
    pub fn declared_cover_assets(&self) -> HashMap<&str, &CoverArtAsset> {
        self.items
            .iter()
            .filter_map(|item| item.cover.as_ref())
            .map(|asset| (asset.path.as_str(), asset))
            .collect()
    }

    /// Every declared asset (audio and cover art) keyed by canonical path. This
    /// is the closed-world enumeration used by archive staging and installed-
    /// pack verification, so undeclared files and missing covers both fail.
    pub fn all_declared_assets(&self) -> HashMap<&str, DeclaredAsset<'_>> {
        let mut assets = HashMap::new();
        for item in &self.items {
            for variant in &item.variants {
                assets.insert(
                    variant.asset.path.as_str(),
                    DeclaredAsset::Audio(&variant.asset),
                );
            }
            if let Some(cover) = &item.cover {
                assets.insert(cover.path.as_str(), DeclaredAsset::Cover(cover));
            }
        }
        assets
    }
}

impl GeneratedLocalRecord {
    pub fn validate(&self) -> Result<(), ManifestValidationError> {
        let mut issues = Vec::new();
        if self.evidence.producer != "adhd-music-studio" {
            issues.push("generated-local evidence producer is not app-owned".to_owned());
        }
        if self.evidence.job_id != self.generation_job_id
            || self.evidence.completed_at_unix_seconds <= 0
        {
            issues.push("generated-local evidence is invalid".to_owned());
        }
        if let Err(error) = self
            .manifest
            .validate_generated_local(&self.generation_job_id)
        {
            issues.extend(error.0);
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestValidationError(issues))
        }
    }
}

pub fn canonical_manifest_bytes(
    manifest: &ContentPackManifest,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&manifest.canonicalized())
}

fn stimulation_order(level: StimulationAvailability) -> u8 {
    match level {
        StimulationAvailability::Off => 0,
        StimulationAvailability::Low => 1,
        StimulationAvailability::Medium => 2,
        StimulationAvailability::High => 3,
    }
}

fn validate_taxonomy(
    kind: &str,
    terms: &[TaxonomyTerm],
    issues: &mut Vec<String>,
) -> HashSet<String> {
    if terms.is_empty() {
        issues.push(format!("taxonomy.{kind}s must not be empty"));
    }
    let mut ids = HashSet::new();
    for term in terms {
        validate_id(&format!("{kind}.id"), &term.id, issues);
        required(&format!("{kind}.label"), &term.label, issues);
        if !ids.insert(term.id.clone()) {
            issues.push(format!("duplicate {kind} id {}", term.id));
        }
    }
    ids
}

fn validate_item(
    item: &ContentItem,
    format_version: u32,
    genre_ids: &HashSet<String>,
    mood_ids: &HashSet<String>,
    item_ids: &mut HashSet<String>,
    asset_paths: &mut HashSet<String>,
    issues: &mut Vec<String>,
) {
    let prefix = format!("item {}", item.id);
    validate_id("item.id", &item.id, issues);
    required(&format!("{prefix}.title"), &item.title, issues);
    if !item_ids.insert(item.id.clone()) {
        issues.push(format!("duplicate item id {}", item.id));
    }
    validate_tags(&prefix, "genre", &item.genre_ids, genre_ids, issues);
    validate_tags(&prefix, "mood", &item.mood_ids, mood_ids, issues);
    validate_activities(item, issues);
    validate_provenance(item, issues);
    validate_analysis(item, issues);
    validate_human_qa(item, issues);

    if item.variants.is_empty() {
        issues.push(format!("{prefix} must declare at least one variant"));
    }
    let mut variant_ids = HashSet::new();
    for variant in &item.variants {
        validate_id("variant.id", &variant.id, issues);
        if !variant_ids.insert(variant.id.clone()) {
            issues.push(format!("{prefix} has duplicate variant id {}", variant.id));
        }
        validate_asset(
            &prefix,
            format_version,
            variant,
            asset_paths,
            item.analysis.duration_seconds,
            issues,
        );
    }
    validate_cover_asset(&prefix, item, asset_paths, issues);
}

fn validate_item_without_human_qa(
    item: &ContentItem,
    format_version: u32,
    genre_ids: &HashSet<String>,
    mood_ids: &HashSet<String>,
    item_ids: &mut HashSet<String>,
    asset_paths: &mut HashSet<String>,
    issues: &mut Vec<String>,
) {
    let prefix = format!("item {}", item.id);
    validate_id("item.id", &item.id, issues);
    required(&format!("{prefix}.title"), &item.title, issues);
    if !item_ids.insert(item.id.clone()) {
        issues.push(format!("duplicate item id {}", item.id));
    }
    validate_tags(&prefix, "genre", &item.genre_ids, genre_ids, issues);
    validate_tags(&prefix, "mood", &item.mood_ids, mood_ids, issues);
    validate_activities(item, issues);
    validate_provenance(item, issues);
    validate_analysis(item, issues);
    if item.variants.is_empty() {
        issues.push(format!("{prefix} must declare at least one variant"));
    }
    let mut variant_ids = HashSet::new();
    for variant in &item.variants {
        validate_id("variant.id", &variant.id, issues);
        if !variant_ids.insert(variant.id.clone()) {
            issues.push(format!("{prefix} has duplicate variant id {}", variant.id));
        }
        validate_asset(
            &prefix,
            format_version,
            variant,
            asset_paths,
            item.analysis.duration_seconds,
            issues,
        );
    }
    validate_cover_asset(&prefix, item, asset_paths, issues);
}

fn validate_tags(
    prefix: &str,
    kind: &str,
    values: &[String],
    known: &HashSet<String>,
    issues: &mut Vec<String>,
) {
    if values.is_empty() {
        issues.push(format!("{prefix} must reference at least one {kind}"));
    }
    let mut seen = HashSet::new();
    for value in values {
        if !known.contains(value) {
            issues.push(format!("{prefix} references unknown {kind} {value}"));
        }
        if !seen.insert(value) {
            issues.push(format!("{prefix} repeats {kind} {value}"));
        }
    }
}

fn validate_activities(item: &ContentItem, issues: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for entry in &item.activity_suitability {
        if !seen.insert(entry.activity) {
            issues.push(format!("item {} repeats an activity suitability", item.id));
        }
        bounded(
            &format!("item {} activity suitability", item.id),
            entry.suitability,
            0.0,
            1.0,
            issues,
        );
    }
    for activity in [
        Activity::DeepWork,
        Activity::Motivation,
        Activity::Creativity,
        Activity::Learning,
        Activity::LightWork,
    ] {
        if !seen.contains(&activity) {
            issues.push(format!(
                "item {} has no suitability for {}",
                item.id,
                activity.storage_key()
            ));
        }
    }
}

fn validate_provenance(item: &ContentItem, issues: &mut Vec<String>) {
    let provenance = &item.provenance;
    required("provenance.source", &provenance.source, issues);
    required("provenance.licence_id", &provenance.licence_id, issues);
    if provenance
        .composer
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && provenance.generator.is_none()
    {
        issues.push(format!("item {} needs a composer or generator", item.id));
    }
    if let Some(generator) = &provenance.generator {
        required("generator.provider", &generator.provider, issues);
        required("generator.model", &generator.model, issues);
        required("generator.model_version", &generator.model_version, issues);
        required("generator.prompt", &generator.prompt, issues);
    }
    if provenance.contains_lyrics {
        issues.push(format!("item {} contains lyrics", item.id));
    }
    if provenance.contains_speech {
        issues.push(format!("item {} contains speech", item.id));
    }
}

fn validate_analysis(item: &ContentItem, issues: &mut Vec<String>) {
    use calibration_policy::*;
    let a = &item.analysis;
    bounded(
        "duration_seconds",
        a.duration_seconds,
        MIN_TRACK_SECONDS,
        MAX_TRACK_SECONDS,
        issues,
    );
    bounded(
        "integrated_lufs",
        a.integrated_lufs,
        MIN_INTEGRATED_LUFS,
        MAX_INTEGRATED_LUFS,
        issues,
    );
    bounded(
        "true_peak_dbfs",
        a.true_peak_dbfs,
        MIN_TRUE_PEAK_DBFS,
        MAX_TRUE_PEAK_DBFS,
        issues,
    );
    bounded(
        "loudness_range_lu",
        a.loudness_range_lu,
        0.0,
        MAX_LOUDNESS_RANGE_LU,
        issues,
    );
    bounded(
        "spectral_centroid_hz",
        a.spectral_centroid_hz,
        0.0,
        MAX_SPECTRAL_CENTROID_HZ,
        issues,
    );
    bounded(
        "high_frequency_energy_ratio",
        a.high_frequency_energy_ratio,
        0.0,
        1.0,
        issues,
    );
    bounded(
        "onset_density_per_second",
        a.onset_density_per_second,
        0.0,
        MAX_ONSET_DENSITY_PER_SECOND,
        issues,
    );
    bounded(
        "tempo_bpm",
        a.tempo_bpm,
        MIN_TEMPO_BPM,
        MAX_TEMPO_BPM,
        issues,
    );
    bounded("tempo_confidence", a.tempo_confidence, 0.0, 1.0, issues);
    bounded(
        "tempo_drift_percent",
        a.tempo_drift_percent,
        0.0,
        MAX_TEMPO_DRIFT_PERCENT,
        issues,
    );
    bounded(
        "section_change_novelty",
        a.section_change_novelty,
        0.0,
        1.0,
        issues,
    );
    bounded(
        "unexplained_silence_seconds",
        a.unexplained_silence_seconds,
        0.0,
        a.duration_seconds,
        issues,
    );
    bounded(
        "vocal_speech_likelihood",
        a.vocal_speech_likelihood,
        0.0,
        1.0,
        issues,
    );
    if a.vocal_speech_likelihood > MAX_VOCAL_SPEECH_LIKELIHOOD {
        issues.push(format!(
            "item {} vocal/speech likelihood exceeds calibration policy",
            item.id
        ));
    }
    if a.unexplained_silence_seconds > 0.0 {
        issues.push(format!("item {} has unexplained silence", item.id));
    }
    if a.clipped_samples > 0 {
        issues.push(format!("item {} has clipped samples", item.id));
    }
    if a.discontinuity_detected {
        issues.push(format!("item {} has a discontinuity", item.id));
    }
    if a.codec_errors_detected || a.corruption_detected {
        issues.push(format!("item {} has codec/corruption errors", item.id));
    }
}

fn validate_human_qa(item: &ContentItem, issues: &mut Vec<String>) {
    use calibration_policy::MIN_HUMAN_REVIEWS;
    if item.human_qa.status != HumanQaStatus::Approved {
        issues.push(format!("item {} is not human-approved", item.id));
    }
    if item.human_qa.reviews.len() < MIN_HUMAN_REVIEWS {
        issues.push(format!(
            "item {} needs at least {MIN_HUMAN_REVIEWS} human reviews",
            item.id
        ));
    }
    let mut reviewers = HashSet::new();
    for review in &item.human_qa.reviews {
        validate_id("reviewer_id", &review.reviewer_id, issues);
        required("review.reviewed_at", &review.reviewed_at, issues);
        required("review.notes", &review.notes, issues);
        if !reviewers.insert(&review.reviewer_id) {
            issues.push(format!("item {} repeats a reviewer", item.id));
        }
    }
    if !item
        .human_qa
        .reviews
        .iter()
        .any(|review| review.representative_work_session)
    {
        issues.push(format!(
            "item {} lacks representative-work listening QA",
            item.id
        ));
    }
}

fn validate_asset(
    prefix: &str,
    format_version: u32,
    variant: &ContentVariant,
    paths: &mut HashSet<String>,
    duration: f32,
    issues: &mut Vec<String>,
) {
    let asset = &variant.asset;
    match format_version {
        FORMAT_VERSION if asset.codec == AssetCodec::OggOpus => issues.push(format!(
            "format_version 1 asset {} may not use Ogg Opus",
            asset.path
        )),
        FORMAT_VERSION_2 if asset.codec != AssetCodec::OggOpus => issues.push(format!(
            "format_version 2 asset {} must use Ogg Opus",
            asset.path
        )),
        _ => {}
    }
    let normalized = canonical_pack_path(&asset.path);
    if normalized
        .as_deref()
        .is_none_or(|path| !path.starts_with("assets/"))
    {
        issues.push(format!("{prefix} has invalid asset path {}", asset.path));
    }
    let duplicate_key = normalized
        .as_deref()
        .unwrap_or(&asset.path)
        .to_ascii_lowercase();
    if !paths.insert(duplicate_key) {
        issues.push(format!("duplicate asset path {}", asset.path));
    }
    if asset.sha256.len() != 64
        || !asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        issues.push(format!("asset {} has invalid SHA-256", asset.path));
    }
    if asset.bytes == 0 {
        issues.push(format!("asset {} has zero bytes", asset.path));
    }
    let extension = asset.path.rsplit('.').next().unwrap_or_default();
    if !extension.eq_ignore_ascii_case(asset.codec.expected_extension()) {
        issues.push(format!(
            "asset {} extension does not match codec",
            asset.path
        ));
    }
    if !(8_000..=192_000).contains(&asset.sample_rate_hz) {
        issues.push(format!("asset {} has invalid sample rate", asset.path));
    }
    if !(1..=8).contains(&asset.channels) {
        issues.push(format!("asset {} has invalid channel count", asset.path));
    }
    if asset
        .bit_depth
        .is_some_and(|depth| !matches!(depth, 16 | 24 | 32))
    {
        issues.push(format!("asset {} has invalid bit depth", asset.path));
    }
    if asset.codec == AssetCodec::OggOpus {
        if asset.sample_rate_hz != 48_000 {
            issues.push(format!(
                "Ogg Opus asset {} must declare the 48000 Hz decode rate",
                asset.path
            ));
        }
        if !matches!(asset.channels, 1 | 2) {
            issues.push(format!(
                "Ogg Opus asset {} must be mono or stereo",
                asset.path
            ));
        }
        if asset.bit_depth.is_some() {
            issues.push(format!(
                "Ogg Opus asset {} must not declare a PCM bit depth",
                asset.path
            ));
        }
    }
    if variant.safe_regions.is_empty() {
        issues.push(format!(
            "variant {} has no authored safe region",
            variant.id
        ));
    }
    for region in &variant.safe_regions {
        if !region.start_seconds.is_finite()
            || !region.end_seconds.is_finite()
            || region.start_seconds < 0.0
            || region.end_seconds <= region.start_seconds
            || region.end_seconds > duration
        {
            issues.push(format!("variant {} has an invalid safe region", variant.id));
        }
    }
    let levels: HashSet<_> = variant.stimulation_available.iter().copied().collect();
    if levels.len() != variant.stimulation_available.len()
        || !levels.contains(&StimulationAvailability::Off)
    {
        issues.push(format!(
            "variant {} stimulation availability must be unique and include Off",
            variant.id
        ));
    }
}

/// Largest accepted cover-art payload. Keeps the renderer-bound data URL and
/// installed-pack verification bounded; cover art is decorative, not audio.
pub const MAX_COVER_ART_BYTES: u64 = 4 * 1024 * 1024;
/// Cover-art images are decorative; clamp dimensions to a sane display bound.
pub const MAX_COVER_ART_DIMENSION: u32 = 4096;

fn validate_cover_asset(
    prefix: &str,
    item: &ContentItem,
    paths: &mut HashSet<String>,
    issues: &mut Vec<String>,
) {
    let Some(cover) = &item.cover else {
        return;
    };
    let normalized = canonical_pack_path(&cover.path);
    if normalized
        .as_deref()
        .is_none_or(|path| !path.starts_with("assets/"))
    {
        issues.push(format!("{prefix} has invalid cover path {}", cover.path));
    }
    let duplicate_key = normalized
        .as_deref()
        .unwrap_or(&cover.path)
        .to_ascii_lowercase();
    if !paths.insert(duplicate_key) {
        issues.push(format!("duplicate asset path {}", cover.path));
    }
    if cover.sha256.len() != 64
        || !cover
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        issues.push(format!("cover {} has invalid SHA-256", cover.path));
    }
    if cover.bytes == 0 {
        issues.push(format!("cover {} has zero bytes", cover.path));
    } else if cover.bytes > MAX_COVER_ART_BYTES {
        issues.push(format!("cover {} exceeds the byte bound", cover.path));
    }
    let extension = cover.path.rsplit('.').next().unwrap_or_default();
    if !extension.eq_ignore_ascii_case(cover.format.expected_extension()) {
        issues.push(format!(
            "cover {} extension does not match format",
            cover.path
        ));
    }
    if cover.width == 0
        || cover.height == 0
        || cover.width > MAX_COVER_ART_DIMENSION
        || cover.height > MAX_COVER_ART_DIMENSION
    {
        issues.push(format!("cover {} has invalid dimensions", cover.path));
    }
    validate_cover_provenance(prefix, cover, issues);
}

fn validate_cover_provenance(prefix: &str, cover: &CoverArtAsset, issues: &mut Vec<String>) {
    let provenance = &cover.provenance;
    required(
        &format!("{prefix} cover source"),
        &provenance.source,
        issues,
    );
    if provenance.generator.is_none() && provenance.licence_id.is_none() {
        issues.push(format!(
            "{prefix} cover {} must declare a generator or licence",
            cover.path
        ));
    }
    if let Some(generator) = &provenance.generator {
        required("cover generator.provider", &generator.provider, issues);
        required("cover generator.model", &generator.model, issues);
        required(
            "cover generator.model_version",
            &generator.model_version,
            issues,
        );
        required("cover generator.prompt", &generator.prompt, issues);
    }
    if let Some(licence_url) = provenance.licence_url.as_deref() {
        if licence_url.trim().is_empty() {
            issues.push(format!(
                "{prefix} cover {} has an empty licence url",
                cover.path
            ));
        }
    }
}

pub const MAX_PACK_PATH_BYTES: usize = 240;
pub const MAX_PACK_PATH_SEGMENT_BYTES: usize = 64;

/// Returns the single platform-neutral spelling accepted by manifests,
/// archives, ingest, and installed-tree verification. The grammar is ASCII so
/// Unicode normalization and platform-specific case folding cannot create
/// aliases after validation.
pub fn canonical_pack_path(path: &str) -> Option<String> {
    if path.is_empty() || path.len() > MAX_PACK_PATH_BYTES || !path.is_ascii() {
        return None;
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment.len() > MAX_PACK_PATH_SEGMENT_BYTES
            || segment == "."
            || segment == ".."
            || segment.starts_with('.')
            || segment.ends_with('.')
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || is_windows_reserved_segment(segment)
        {
            return None;
        }
    }
    Some(path.to_owned())
}

pub fn valid_pack_path(path: &str) -> bool {
    canonical_pack_path(path).is_some()
}

fn is_windows_reserved_segment(segment: &str) -> bool {
    let basename = segment.split('.').next().unwrap_or_default();
    let upper = basename.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}

pub fn is_stable_identifier(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'_' | b'-' => index > 0,
            _ => false,
        })
}

fn validate_id(field: &str, value: &str, issues: &mut Vec<String>) {
    if !is_stable_identifier(value) {
        issues.push(format!("{field} is not a stable lowercase ID: {value}"));
    }
}

fn required(field: &str, value: &str, issues: &mut Vec<String>) {
    if value.trim().is_empty() {
        issues.push(format!("{field} is required"));
    }
}

fn bounded(field: &str, value: f32, min: f32, max: f32, issues: &mut Vec<String>) {
    if !value.is_finite() || value < min || value > max {
        issues.push(format!("{field} must be finite and within {min}..={max}"));
    }
}
