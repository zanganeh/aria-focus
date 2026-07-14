//! Candidate-ledger schema for planned ACE-Step generation batches.
//!
//! The ledger captures the *planned* inputs for a deterministic generation
//! run against pinned ACE-Step 1.5 artifacts. It never stores generated audio
//! hashes or post-generation evidence: those belong to the published catalogue.
//! This crate performs strict JSON deserialization (rejecting duplicate object
//! keys and unknown fields), validates a versioned planned batch against the
//! pinned model identifiers and instrumental-only policy, and emits
//! deterministic canonical JSON for reproducible round-tripping.

use std::collections::HashSet;
use std::fmt;

use domain::Activity;
use serde::de::{DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

/// Schema identifier expected on every planned batch.
pub const SCHEMA_NAME: &str = "adhd-music.candidate-ledger.planned";
/// Schema version supported by this crate.
pub const SCHEMA_VERSION: u32 = 1;

/// Pinned ACE-Step 1.5 generator/model identifiers.
pub mod pins {
    pub const CONFIG: &str = "acestep-v15-turbo";
    pub const PLANNER: &str = "acestep-5Hz-lm-0.6B";
    pub const PYTHON_VERSION: &str = "3.12";
    pub const SOURCE_URL: &str = "https://github.com/ace-step/ACE-Step-1.5.git";
    pub const SOURCE_COMMIT: &str = "6d467e4b5081ccb0abf1ec1bf4fdf9051a2d34b0";
    pub const TURBO_VAE_REPO: &str = "ACE-Step/Ace-Step1.5";
    pub const TURBO_VAE_REVISION: &str = "19671f406d603126926c1b7e2adc169acbcade22";
    pub const PLANNER_REPO: &str = "ACE-Step/acestep-5Hz-lm-0.6B";
    pub const PLANNER_REVISION: &str = "148d8ea0225bdab342ee1ae3a354275ccd60ca80";
}

/// Required inference configuration for every planned candidate.
pub mod inference {
    pub const SAMPLE_RATE_HZ: u32 = 48_000;
    pub const STEPS: u32 = 8;
    pub const SHIFT: u32 = 3;
}

/// Output-licence identifiers that may be claimed for generated audio. CC0 is
/// deliberately excluded: ACE-Step outputs are not public-domain.
pub const ALLOWED_OUTPUT_LICENCES: &[&str] = &["ace-step-1.5-output-terms"];

/// Literal that must appear in every positive prompt to enforce instrumental
/// generation only.
pub const INSTRUMENTAL_MARKER: &str = "[Instrumental]";

const DURATION_MIN: f32 = 10.0;
const DURATION_MAX: f32 = 600.0;
const BPM_MIN: u32 = 40;
const BPM_MAX: u32 = 200;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("candidate-ledger validation failed: {issues}", issues = .0.join("; "))]
pub struct ValidationError(pub Vec<String>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedBatch {
    pub schema: String,
    pub schema_version: u32,
    pub batch: BatchMetadata,
    pub taxonomy: Taxonomy,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchMetadata {
    pub id: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub generator_pin: GeneratorPin,
    pub terms_evidence: TermsEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorPin {
    pub config: String,
    pub planner: String,
    pub python_version: String,
    pub source_url: String,
    pub source_commit: String,
    pub turbo_vae_repo: String,
    pub turbo_vae_revision: String,
    pub planner_repo: String,
    pub planner_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermsEvidence {
    pub licence_id: String,
    pub licence_url: String,
    pub model_card_url: String,
    pub output_licence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_archive_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taxonomy {
    pub activities: Vec<Activity>,
    pub genres: Vec<TaxonomyTerm>,
    pub moods: Vec<TaxonomyTerm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyTerm {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub seed: u64,
    pub activity: Activity,
    pub genre_ids: Vec<String>,
    pub mood_ids: Vec<String>,
    pub duration_seconds: f32,
    pub bpm: u32,
    pub contains_lyrics: bool,
    pub contains_speech: bool,
    pub prompts: Prompts,
    pub inference: InferenceParams,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompts {
    pub positive: String,
    pub negative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Flac,
    Wav,
    Mp3,
    OggOpus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Solver {
    Ode,
    Euler,
    Dpm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceParams {
    pub codec: Codec,
    pub sample_rate_hz: u32,
    pub steps: u32,
    pub shift: u32,
    pub solver: Solver,
    pub use_random_seed: bool,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub value: String,
}

impl PlannedBatch {
    /// Validate a versioned planned batch against the pinned ACE-Step 1.5
    /// artifacts and instrumental-only policy.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut issues = Vec::new();
        if self.schema != SCHEMA_NAME {
            issues.push(format!("schema must be {SCHEMA_NAME}"));
        }
        if self.schema_version != SCHEMA_VERSION {
            issues.push(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        validate_batch(&self.batch, &mut issues);
        validate_taxonomy(&self.taxonomy, &mut issues);
        validate_candidates(&self.candidates, &self.taxonomy, &mut issues);
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ValidationError(issues))
        }
    }

    /// Return a canonicalized copy with collections sorted for deterministic
    /// JSON emission. Validation requirements (e.g. ordered parameter names)
    /// are preserved; this additionally normalizes taxonomy and candidate order.
    pub fn canonicalized(&self) -> Self {
        let mut batch = self.clone();
        batch
            .taxonomy
            .activities
            .sort_by(|a, b| a.storage_key().cmp(b.storage_key()));
        batch.taxonomy.genres.sort_by(|a, b| a.id.cmp(&b.id));
        batch.taxonomy.moods.sort_by(|a, b| a.id.cmp(&b.id));
        batch.candidates.sort_by(|a, b| a.id.cmp(&b.id));
        for candidate in &mut batch.candidates {
            candidate.genre_ids.sort();
            candidate.mood_ids.sort();
            candidate
                .inference
                .parameters
                .sort_by(|a, b| a.name.cmp(&b.name));
        }
        batch
    }
}

/// Serialize a canonicalized planned batch to deterministic JSON bytes.
pub fn canonical_bytes(batch: &PlannedBatch) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&batch.canonicalized())
}

/// Strictly deserialize a planned batch from a JSON string, rejecting duplicate
/// object keys at any depth and unknown fields via `#[serde(deny_unknown_fields)]`.
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let strict = StrictValue::deserialize(&mut deserializer)?;
    deserializer.end()?;
    serde_json::from_value(strict_value_to_json(strict))
}

/// Strictly deserialize a planned batch from JSON bytes, rejecting duplicate
/// object keys at any depth and unknown fields.
pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let strict = StrictValue::deserialize(&mut deserializer)?;
    deserializer.end()?;
    serde_json::from_value(strict_value_to_json(strict))
}

fn validate_batch(batch: &BatchMetadata, issues: &mut Vec<String>) {
    validate_name("batch.id", &batch.id, issues);
    required("batch.created_at", &batch.created_at, issues);
    validate_generator_pin(&batch.generator_pin, issues);
    validate_terms_evidence(&batch.terms_evidence, issues);
}

fn validate_generator_pin(pin: &GeneratorPin, issues: &mut Vec<String>) {
    if pin.config != pins::CONFIG {
        issues.push(format!("generator_pin.config must be {}", pins::CONFIG));
    }
    if pin.planner != pins::PLANNER {
        issues.push(format!("generator_pin.planner must be {}", pins::PLANNER));
    }
    if pin.python_version != pins::PYTHON_VERSION {
        issues.push(format!(
            "generator_pin.python_version must be {}",
            pins::PYTHON_VERSION
        ));
    }
    if pin.source_url != pins::SOURCE_URL {
        issues.push("generator_pin.source_url mismatch".to_owned());
    }
    if pin.source_commit != pins::SOURCE_COMMIT {
        issues.push("generator_pin.source_commit mismatch".to_owned());
    }
    if pin.turbo_vae_repo != pins::TURBO_VAE_REPO {
        issues.push("generator_pin.turbo_vae_repo mismatch".to_owned());
    }
    if pin.turbo_vae_revision != pins::TURBO_VAE_REVISION {
        issues.push("generator_pin.turbo_vae_revision mismatch".to_owned());
    }
    if pin.planner_repo != pins::PLANNER_REPO {
        issues.push("generator_pin.planner_repo mismatch".to_owned());
    }
    if pin.planner_revision != pins::PLANNER_REVISION {
        issues.push("generator_pin.planner_revision mismatch".to_owned());
    }
}

fn validate_terms_evidence(evidence: &TermsEvidence, issues: &mut Vec<String>) {
    required("terms_evidence.licence_id", &evidence.licence_id, issues);
    required("terms_evidence.licence_url", &evidence.licence_url, issues);
    required(
        "terms_evidence.model_card_url",
        &evidence.model_card_url,
        issues,
    );
    required(
        "terms_evidence.output_licence",
        &evidence.output_licence,
        issues,
    );
    if evidence.output_licence == "CC0" {
        issues.push("terms_evidence.output_licence must not claim CC0".to_owned());
    } else if !ALLOWED_OUTPUT_LICENCES.contains(&evidence.output_licence.as_str()) {
        issues.push(format!(
            "terms_evidence.output_licence {} is not allowed",
            evidence.output_licence
        ));
    }
    if let Some(hash) = &evidence.source_archive_sha256 {
        if !is_canonical_sha256(hash) {
            issues.push(
                "terms_evidence.source_archive_sha256 must be canonical lowercase SHA-256"
                    .to_owned(),
            );
        }
    }
}

fn validate_taxonomy(taxonomy: &Taxonomy, issues: &mut Vec<String>) {
    if taxonomy.activities.is_empty() {
        issues.push("taxonomy.activities must not be empty".to_owned());
    }
    let mut activities = HashSet::new();
    for activity in &taxonomy.activities {
        if !activities.insert(*activity) {
            issues.push(format!(
                "taxonomy.activities repeats {}",
                activity.storage_key()
            ));
        }
    }
    validate_taxonomy_terms("genre", &taxonomy.genres, issues);
    validate_taxonomy_terms("mood", &taxonomy.moods, issues);
}

fn validate_taxonomy_terms(kind: &str, terms: &[TaxonomyTerm], issues: &mut Vec<String>) {
    if terms.is_empty() {
        issues.push(format!("taxonomy.{kind}s must not be empty"));
    }
    let mut ids = HashSet::new();
    for term in terms {
        validate_name(&format!("{kind}.id"), &term.id, issues);
        required(&format!("{kind}.label"), &term.label, issues);
        if !ids.insert(term.id.clone()) {
            issues.push(format!("duplicate {kind} id {}", term.id));
        }
    }
}

fn validate_candidates(candidates: &[Candidate], taxonomy: &Taxonomy, issues: &mut Vec<String>) {
    if candidates.is_empty() {
        issues.push("batch must contain at least one candidate".to_owned());
    }
    let supported: HashSet<Activity> = taxonomy.activities.iter().copied().collect();
    let genre_ids: HashSet<&str> = taxonomy.genres.iter().map(|t| t.id.as_str()).collect();
    let mood_ids: HashSet<&str> = taxonomy.moods.iter().map(|t| t.id.as_str()).collect();
    let mut ids = HashSet::new();
    let mut seeds = HashSet::new();
    for candidate in candidates {
        validate_name("candidate.id", &candidate.id, issues);
        if !ids.insert(candidate.id.clone()) {
            issues.push(format!("duplicate candidate id {}", candidate.id));
        }
        if !seeds.insert(candidate.seed) {
            issues.push(format!(
                "candidate {} reuses seed {}",
                candidate.id, candidate.seed
            ));
        }
        if !supported.contains(&candidate.activity) {
            issues.push(format!(
                "candidate {} has unsupported activity {}",
                candidate.id,
                candidate.activity.storage_key()
            ));
        }
        validate_tag_refs(
            &candidate.id,
            "genre",
            &candidate.genre_ids,
            &genre_ids,
            issues,
        );
        validate_tag_refs(
            &candidate.id,
            "mood",
            &candidate.mood_ids,
            &mood_ids,
            issues,
        );
        if !candidate.duration_seconds.is_finite()
            || candidate.duration_seconds < DURATION_MIN
            || candidate.duration_seconds > DURATION_MAX
        {
            issues.push(format!(
                "candidate {} duration_seconds must be within {}..={}",
                candidate.id, DURATION_MIN, DURATION_MAX
            ));
        }
        if candidate.bpm < BPM_MIN || candidate.bpm > BPM_MAX {
            issues.push(format!(
                "candidate {} bpm must be within {}..={}",
                candidate.id, BPM_MIN, BPM_MAX
            ));
        }
        if candidate.contains_lyrics {
            issues.push(format!("candidate {} declares lyrics", candidate.id));
        }
        if candidate.contains_speech {
            issues.push(format!("candidate {} declares speech", candidate.id));
        }
        if !candidate.prompts.positive.contains(INSTRUMENTAL_MARKER) {
            issues.push(format!(
                "candidate {} positive prompt must contain {INSTRUMENTAL_MARKER}",
                candidate.id
            ));
        }
        required(
            &format!("candidate {} negative prompt", candidate.id),
            &candidate.prompts.negative,
            issues,
        );
        validate_inference(&candidate.id, &candidate.inference, issues);
    }
}

fn validate_tag_refs(
    candidate_id: &str,
    kind: &str,
    values: &[String],
    known: &HashSet<&str>,
    issues: &mut Vec<String>,
) {
    if values.is_empty() {
        issues.push(format!(
            "candidate {candidate_id} must reference at least one {kind}"
        ));
    }
    let mut seen = HashSet::new();
    for value in values {
        if !known.contains(value.as_str()) {
            issues.push(format!(
                "candidate {candidate_id} references unknown {kind} {value}"
            ));
        }
        if !seen.insert(value.clone()) {
            issues.push(format!("candidate {candidate_id} repeats {kind} {value}"));
        }
    }
}

fn validate_inference(candidate_id: &str, inference: &InferenceParams, issues: &mut Vec<String>) {
    if inference.codec != Codec::Flac {
        issues.push(format!(
            "candidate {candidate_id} inference codec must be flac"
        ));
    }
    if inference.sample_rate_hz != inference::SAMPLE_RATE_HZ {
        issues.push(format!(
            "candidate {candidate_id} inference sample_rate_hz must be {}",
            inference::SAMPLE_RATE_HZ
        ));
    }
    if inference.steps != inference::STEPS {
        issues.push(format!(
            "candidate {candidate_id} inference steps must be {}",
            inference::STEPS
        ));
    }
    if inference.shift != inference::SHIFT {
        issues.push(format!(
            "candidate {candidate_id} inference shift must be {}",
            inference::SHIFT
        ));
    }
    if inference.solver != Solver::Ode {
        issues.push(format!(
            "candidate {candidate_id} inference solver must be ode"
        ));
    }
    if inference.use_random_seed {
        issues.push(format!(
            "candidate {candidate_id} inference use_random_seed must be false"
        ));
    }
    validate_parameters(candidate_id, &inference.parameters, issues);
}

fn validate_parameters(candidate_id: &str, parameters: &[Parameter], issues: &mut Vec<String>) {
    let mut seen = HashSet::new();
    let mut prev: Option<&str> = None;
    for parameter in parameters {
        required(
            &format!("candidate {candidate_id} parameter name"),
            &parameter.name,
            issues,
        );
        if !seen.insert(parameter.name.clone()) {
            issues.push(format!(
                "candidate {candidate_id} repeats parameter {}",
                parameter.name
            ));
        }
        if let Some(previous) = prev {
            if parameter.name.as_str() < previous {
                issues.push(format!(
                    "candidate {candidate_id} parameters must be ordered by name"
                ));
            }
        }
        prev = Some(&parameter.name);
    }
}

fn validate_name(field: &str, value: &str, issues: &mut Vec<String>) {
    let len = value.len();
    let valid = (1..=64).contains(&len)
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' => index > 0 && index < len - 1,
            _ => false,
        });
    if !valid {
        issues.push(format!("{field} is not a safe planned name: {value}"));
    }
}

fn required(field: &str, value: &str, issues: &mut Vec<String>) {
    if value.trim().is_empty() {
        issues.push(format!("{field} is required"));
    }
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// ---------------------------------------------------------------------------
// Strict JSON deserialization: rejects duplicate object keys at any depth.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum StrictValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<StrictValue>),
    Object(Vec<(String, StrictValue)>),
}

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<StrictValue, E> {
        Ok(StrictValue::Bool(value))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<StrictValue, E> {
        Ok(StrictValue::Number(serde_json::Number::from(value)))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<StrictValue, E> {
        Ok(StrictValue::Number(serde_json::Number::from(value)))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<StrictValue, E> {
        match serde_json::Number::from_f64(value) {
            Some(number) => Ok(StrictValue::Number(number)),
            None => Err(serde::de::Error::custom("float is not finite")),
        }
    }

    fn visit_i128<E: serde::de::Error>(self, _value: i128) -> Result<StrictValue, E> {
        Err(serde::de::Error::custom(
            "128-bit integers are not supported",
        ))
    }

    fn visit_u128<E: serde::de::Error>(self, _value: u128) -> Result<StrictValue, E> {
        Err(serde::de::Error::custom(
            "128-bit integers are not supported",
        ))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<StrictValue, E> {
        Ok(StrictValue::String(value.to_owned()))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<StrictValue, E> {
        Ok(StrictValue::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<StrictValue, A::Error> {
        let mut out = Vec::new();
        while let Some(element) = seq.next_element::<StrictValue>()? {
            out.push(element);
        }
        Ok(StrictValue::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<StrictValue, A::Error> {
        let mut out = Vec::new();
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON key: {key}"
                )));
            }
            let value = map.next_value::<StrictValue>()?;
            out.push((key, value));
        }
        Ok(StrictValue::Object(out))
    }
}

fn strict_value_to_json(value: StrictValue) -> serde_json::Value {
    match value {
        StrictValue::Null => serde_json::Value::Null,
        StrictValue::Bool(value) => serde_json::Value::Bool(value),
        StrictValue::Number(number) => serde_json::Value::Number(number),
        StrictValue::String(value) => serde_json::Value::String(value),
        StrictValue::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(strict_value_to_json).collect())
        }
        StrictValue::Object(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, value) in pairs {
                map.insert(key, strict_value_to_json(value));
            }
            serde_json::Value::Object(map)
        }
    }
}

// ---------------------------------------------------------------------------
// Immutable generated-record registration.  This deliberately lives beside the
// planned schema so both commands share the same strict JSON boundary.
// ---------------------------------------------------------------------------

const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_ASSET_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECODED_SAMPLES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationEvidence {
    pub schema: String,
    pub schema_version: u32,
    pub candidate_id: String,
    pub generated_at: String,
    pub machine: String,
    pub gpu: String,
    pub output: EvidenceOutput,
    pub analyzer: EvidenceAnalyzer,
    pub evidence_file_name: String,
    pub edit_lineage: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceOutput {
    pub file_name: String,
    pub bytes: u64,
    pub codec: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceAnalyzer {
    pub file_name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedRecord {
    pub schema: String,
    pub schema_version: u32,
    pub lifecycle: String,
    pub candidate: Candidate,
    pub batch: BatchMetadata,
    pub verified: VerifiedAsset,
    pub evidence: RecordedEvidence,
    pub edit_lineage: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedAsset {
    pub file_name: String,
    pub bytes: u64,
    pub codec: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub frames: u64,
    pub duration_seconds: f64,
    pub sha256: String,
    pub analyzer_file_name: String,
    pub analyzer_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedEvidence {
    pub file_name: String,
    pub sha256: String,
    pub generated_at: String,
    pub machine: String,
    pub gpu: String,
}

#[derive(Debug, Deserialize)]
struct AnalyzerBoundary {
    source: AnalyzerSource,
    decode: AnalyzerDecode,
    hard_rejections: Vec<serde_json::Value>,
}
#[derive(Debug, Deserialize)]
struct AnalyzerSource {
    file_name: String,
    bytes: Option<u64>,
    sha256: Option<String>,
}
#[derive(Debug, Deserialize)]
struct AnalyzerDecode {
    status: String,
    codec: Option<String>,
    sample_rate_hz: Option<u32>,
    channels: Option<u16>,
    frames: Option<u64>,
    duration_seconds: Option<f64>,
}

#[derive(Debug)]
struct Captured {
    bytes: Vec<u8>,
    name: String,
    sha256: String,
}
#[derive(Debug)]
struct Decoded {
    frames: u64,
    duration: f64,
    sample_rate: u32,
    channels: u16,
}

/// Run the standalone CLI.  Both subcommands snapshot their inputs before any
/// parsing so a path replacement cannot split validation from hashing.
pub fn run<I>(args: I) -> Result<(), String>
where
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
{
    let args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
    if args.len() < 2 {
        return Err(usage());
    }
    let command = args[1].to_string_lossy();
    let pairs = parse_arguments(
        &args[2..],
        match command.as_ref() {
            "validate-plan" => &["--plan"][..],
            "register-generated" => &[
                "--plan",
                "--candidate",
                "--asset",
                "--analysis",
                "--evidence",
                "--output",
            ][..],
            _ => return Err(usage()),
        },
    )?;
    match command.as_ref() {
        "validate-plan" => {
            let plan = capture(std::path::Path::new(&pairs["--plan"]), MAX_PLAN_BYTES)?;
            parse_plan(&plan.bytes)?;
            Ok(())
        }
        "register-generated" => register(&pairs),
        _ => unreachable!(),
    }
}

fn usage() -> String {
    "usage: candidate-ledger validate-plan --plan <batch.json> | register-generated --plan <batch.json> --candidate <id> --asset <file.flac> --analysis <report.json> --evidence <generation-evidence.json> --output <record.json>".into()
}

fn parse_arguments(
    args: &[std::ffi::OsString],
    allowed: &[&str],
) -> Result<std::collections::BTreeMap<String, std::path::PathBuf>, String> {
    if args.len() != allowed.len() * 2 {
        return Err(usage());
    }
    let mut out = std::collections::BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].to_string_lossy().to_string();
        if !allowed.contains(&key.as_str()) || out.contains_key(&key) {
            return Err(format!("invalid or repeated argument {key}"));
        }
        let value = args[i + 1].to_string_lossy();
        if value.is_empty() || value.starts_with('-') {
            return Err(format!("missing value for {key}"));
        }
        out.insert(key, std::path::PathBuf::from(args[i + 1].clone()));
        i += 2;
    }
    Ok(out)
}

fn register(args: &std::collections::BTreeMap<String, std::path::PathBuf>) -> Result<(), String> {
    let output = &args["--output"];
    for key in ["--plan", "--asset", "--analysis", "--evidence"] {
        if same_path(&args[key], output)? {
            return Err("output must not alias an input".into());
        }
    }
    validate_output_path(output)?;
    let plan = capture(&args["--plan"], MAX_PLAN_BYTES)?;
    let planned = parse_plan(&plan.bytes)?;
    let candidate_id = args["--candidate"].to_string_lossy().to_string();
    if !safe_id(&candidate_id) {
        return Err("candidate must be a safe planned ID".into());
    }
    let candidate = planned
        .candidates
        .iter()
        .find(|c| c.id == candidate_id)
        .cloned()
        .ok_or("candidate not present in plan")?;
    let asset = capture(&args["--asset"], MAX_ASSET_BYTES)?;
    if !safe_basename(&asset.name, "flac") {
        return Err("asset basename is unsafe or is not .flac".into());
    }
    let decoded = decode_flac(&asset.bytes)?;
    if (decoded.duration - f64::from(candidate.duration_seconds)).abs() > 0.15 {
        return Err("decoded duration does not match planned duration".into());
    }
    let report = capture(&args["--analysis"], MAX_JSON_BYTES)?;
    if !safe_basename(&report.name, "json") {
        return Err("analysis basename is unsafe or is not .json".into());
    }
    validate_analyzer(&report, &asset, &decoded)?;
    let evidence_file = capture(&args["--evidence"], MAX_JSON_BYTES)?;
    if !safe_basename(&evidence_file.name, "json") {
        return Err("evidence basename is unsafe or is not .json".into());
    }
    let evidence: GenerationEvidence =
        from_slice(&evidence_file.bytes).map_err(|e| format!("invalid evidence: {e}"))?;
    validate_evidence(
        &evidence,
        &evidence_file,
        &candidate,
        &asset,
        &report,
        &decoded,
    )?;
    let record = GeneratedRecord {
        schema: "adhd-music.candidate-ledger.generated".into(),
        schema_version: 1,
        lifecycle: "generated".into(),
        // The plan is valid before this point, but its tag order is deliberately
        // not semantically significant.  Never mutate the plan; embed the
        // canonical candidate in an immutable record instead.
        candidate: canonical_candidate(candidate),
        batch: planned.batch,
        verified: VerifiedAsset {
            file_name: asset.name,
            bytes: asset.bytes.len() as u64,
            codec: "flac".into(),
            sample_rate_hz: decoded.sample_rate,
            channels: decoded.channels,
            frames: decoded.frames,
            duration_seconds: decoded.duration,
            sha256: asset.sha256,
            analyzer_file_name: report.name,
            analyzer_sha256: report.sha256,
        },
        evidence: RecordedEvidence {
            file_name: evidence_file.name,
            sha256: evidence_file.sha256,
            generated_at: evidence.generated_at,
            machine: evidence.machine,
            gpu: evidence.gpu,
        },
        edit_lineage: Vec::new(),
    };
    let mut bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    publish_noclobber(output, &bytes)
}

fn parse_plan(bytes: &[u8]) -> Result<PlannedBatch, String> {
    let plan: PlannedBatch = from_slice(bytes).map_err(|e| format!("invalid plan JSON: {e}"))?;
    plan.validate().map_err(|e| e.to_string())?;
    Ok(plan)
}

fn capture(path: &std::path::Path, max: usize) -> Result<Captured, String> {
    capture_inner(path, max, || {})
}

/// Snapshot bytes from one opened regular-file handle.  The before/opened/after
/// metadata comparison closes the replacement window between the pathname
/// check and the open.  It also rejects an in-place length change while read.
fn capture_inner<F: FnOnce()>(
    path: &std::path::Path,
    max: usize,
    after_open: F,
) -> Result<Captured, String> {
    let before = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    validate_safe_regular(&before, path)?;
    if before.len() > max as u64 {
        return Err(format!("{} exceeds byte limit", path.display()));
    }
    let before_id = path_identity(path, &before)?;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    after_open();
    let opened = file
        .metadata()
        .map_err(|e| format!("cannot stat opened {}: {e}", path.display()))?;
    validate_safe_regular(&opened, path)?;
    if opened.len() > max as u64
        || file_identity_from_file(&file, &opened)? != before_id
        || opened.len() != before.len()
    {
        return Err(format!("{} changed while opening", path.display()));
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    use std::io::Read;
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let opened_after = file
        .metadata()
        .map_err(|e| format!("cannot re-stat opened {}: {e}", path.display()))?;
    let after = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot re-stat {}: {e}", path.display()))?;
    validate_safe_regular(&opened_after, path)?;
    validate_safe_regular(&after, path)?;
    if bytes.len() > max
        || opened_after.len() != opened.len()
        || opened_after.len() != bytes.len() as u64
        || file_identity_from_file(&file, &opened_after)? != before_id
        || path_identity(path, &after)? != before_id
        || after.len() != opened.len()
    {
        return Err(format!("{} changed while being captured", path.display()));
    }
    use sha2::Digest;
    let sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
    let name = path
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or("input needs a UTF-8 basename")?
        .to_owned();
    Ok(Captured {
        bytes,
        name,
        sha256,
    })
}

fn validate_safe_regular(
    metadata: &std::fs::Metadata,
    path: &std::path::Path,
) -> Result<(), String> {
    if metadata.file_type().is_symlink() || !metadata.is_file() || is_reparse(metadata) {
        return Err(format!("{} is not a safe regular file", path.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn file_identity(m: &std::fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}
#[cfg(windows)]
fn file_identity_from_file(
    file: &std::fs::File,
    _: &std::fs::Metadata,
) -> Result<FileIdentity, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(format!(
            "cannot identify opened file: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((
        info.dwVolumeSerialNumber,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}

#[cfg(unix)]
fn file_identity_from_file(
    _: &std::fs::File,
    metadata: &std::fs::Metadata,
) -> Result<FileIdentity, String> {
    Ok(file_identity(metadata))
}
#[cfg(unix)]
fn path_identity(
    _: &std::path::Path,
    metadata: &std::fs::Metadata,
) -> Result<FileIdentity, String> {
    Ok(file_identity(metadata))
}
#[cfg(windows)]
fn path_identity(path: &std::path::Path, _: &std::fs::Metadata) -> Result<FileIdentity, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("cannot identify {}: {e}", path.display()))?;
    file_identity_from_file(&file, &file.metadata().map_err(|e| e.to_string())?)
}

#[cfg(windows)]
fn is_reparse(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &std::fs::Metadata) -> bool {
    false
}

fn decode_flac(bytes: &[u8]) -> Result<Decoded, String> {
    use symphonia::core::{
        codecs::audio::{well_known::CODEC_ID_FLAC, AudioDecoderOptions},
        formats::{probe::Hint, FormatOptions, TrackType},
        io::MediaSourceStream,
        meta::MetadataOptions,
    };
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("not FLAC: {e}"))?;
    if format.format_info().short_name != "flac" {
        return Err("captured content is not FLAC".into());
    }
    let (track_id, params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("FLAC has no default track")?;
        (
            track.id,
            track
                .codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .ok_or("FLAC lacks audio parameters")?
                .clone(),
        )
    };
    if params.codec != CODEC_ID_FLAC {
        return Err("captured content is not FLAC".into());
    }
    let sample_rate = params.sample_rate.ok_or("FLAC lacks sample rate")?;
    let channels = params
        .channels
        .as_ref()
        .ok_or("FLAC lacks channels")?
        .count() as u16;
    if sample_rate != 48_000 || channels != 2 {
        return Err("FLAC must be 48 kHz stereo".into());
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut frames = 0u64;
    let mut samples = 0u64;
    loop {
        match format.next_packet() {
            Ok(Some(packet)) => {
                if packet.track_id != track_id {
                    continue;
                }
                let audio = decoder
                    .decode(&packet)
                    .map_err(|e| format!("FLAC decode failed: {e}"))?;
                let mut buf = vec![0.0f32; audio.samples_interleaved()];
                audio.copy_to_slice_interleaved(&mut buf);
                for sample in &buf {
                    if !sample.is_finite() {
                        return Err("FLAC contains non-finite PCM".into());
                    }
                }
                samples += buf.len() as u64;
                if samples > MAX_DECODED_SAMPLES {
                    return Err("decoded sample limit exceeded".into());
                }
                frames += (buf.len() / 2) as u64;
            }
            Ok(None) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                return Err("FLAC decoder reset required".into())
            }
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(e) => return Err(format!("FLAC decode failed: {e}")),
        }
    }
    if frames == 0 {
        return Err("FLAC has no PCM frames".into());
    }
    Ok(Decoded {
        frames,
        duration: frames as f64 / f64::from(sample_rate),
        sample_rate,
        channels,
    })
}

fn validate_analyzer(report: &Captured, asset: &Captured, decoded: &Decoded) -> Result<(), String> {
    let a: AnalyzerBoundary =
        from_slice(&report.bytes).map_err(|e| format!("invalid analyzer JSON: {e}"))?;
    if !a.hard_rejections.is_empty()
        || a.decode.status != "decoded"
        || a.decode.codec.as_deref() != Some("flac")
        || a.decode.sample_rate_hz != Some(decoded.sample_rate)
        || a.decode.channels != Some(decoded.channels)
        || a.decode.frames != Some(decoded.frames)
        || a.decode
            .duration_seconds
            .is_none_or(|v| (v - decoded.duration).abs() > 0.000_001)
        || a.source.file_name != asset.name
        || a.source.bytes != Some(asset.bytes.len() as u64)
        || a.source.sha256.as_deref() != Some(&asset.sha256)
    {
        return Err("analyzer report does not verify captured asset".into());
    }
    Ok(())
}

fn validate_evidence(
    e: &GenerationEvidence,
    file: &Captured,
    c: &Candidate,
    asset: &Captured,
    report: &Captured,
    d: &Decoded,
) -> Result<(), String> {
    if e.schema != "adhd-music.candidate-ledger.generation-evidence"
        || e.schema_version != 1
        || e.candidate_id != c.id
        || !safe_id(&e.candidate_id)
        || e.machine.trim().is_empty()
        || e.gpu.trim().is_empty()
        || !e.edit_lineage.is_empty()
    {
        return Err("invalid generation evidence identity or lineage".into());
    }
    time::OffsetDateTime::parse(
        &e.generated_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| "generated_at must be RFC3339".to_owned())?
    .offset()
    .whole_seconds()
    .eq(&0)
    .then_some(())
    .ok_or("generated_at must be UTC")?;
    if e.output.file_name != asset.name
        || e.output.bytes != asset.bytes.len() as u64
        || e.output.codec != "flac"
        || e.output.sample_rate_hz != d.sample_rate
        || e.output.channels != d.channels
        || e.output.sha256 != asset.sha256
        || e.analyzer.file_name != report.name
        || e.analyzer.sha256 != report.sha256
        || e.evidence_file_name != file.name
        || !safe_basename(&e.output.file_name, "flac")
        || !safe_basename(&e.analyzer.file_name, "json")
        || !safe_basename(&e.evidence_file_name, "json")
        || !is_canonical_sha256(&e.output.sha256)
        || !is_canonical_sha256(&e.analyzer.sha256)
    {
        return Err("generation evidence does not match captured facts".into());
    }
    Ok(())
}

fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().enumerate().all(|(i, b)| {
            matches!(b,b'a'..=b'z'|b'0'..=b'9') || (b == b'-' && i > 0 && i + 1 < s.len())
        })
}
fn safe_basename(s: &str, ext: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if s.is_empty()
        || s != std::path::Path::new(s)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("")
        || !lower.ends_with(&format!(".{ext}"))
        || s.ends_with(['.', ' '])
        || s.bytes().any(|b| {
            b < 32
                || matches!(
                    b,
                    b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
                )
        })
    {
        return false;
    }
    !matches!(
        s.split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
            .as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}
fn same_path(a: &std::path::Path, b: &std::path::Path) -> Result<bool, String> {
    let aa = std::fs::canonicalize(a).map_err(|e| e.to_string())?;
    if b.exists() {
        return Ok(aa == std::fs::canonicalize(b).map_err(|e| e.to_string())?);
    }
    let absolute_b = if b.is_absolute() {
        b.to_path_buf()
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(b)
    };
    Ok(aa == absolute_b)
}
fn validate_output_path(p: &std::path::Path) -> Result<(), String> {
    if !safe_basename(p.file_name().and_then(|s| s.to_str()).unwrap_or(""), "json") {
        return Err("unsafe output basename".into());
    }
    if p.exists() {
        return Err("output already exists".into());
    }
    let parent = p.parent().ok_or("output needs a parent")?;
    let m = std::fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if m.file_type().is_symlink() || is_reparse(&m) || !m.is_dir() {
        return Err("unsafe output parent".into());
    }
    Ok(())
}
fn publish_noclobber(output: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = output.parent().ok_or("output needs parent")?;
    let parent_handle = open_safe_output_directory(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    if !same_output_directory(parent, &parent_handle)? {
        return Err("output directory changed during temp creation".into());
    }
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    if output.exists() {
        return Err("output already exists".into());
    }
    if !same_output_directory(parent, &parent_handle)? {
        return Err("output directory changed before publication".into());
    }
    temp.persist_noclobber(output)
        .map_err(|e| format!("atomic publication failed: {}", e.error))?;
    Ok(())
}

#[cfg(unix)]
struct OutputDirectoryHandle {
    _handle: std::fs::File,
    identity: FileIdentity,
}

#[cfg(unix)]
fn open_safe_output_directory(path: &std::path::Path) -> Result<OutputDirectoryHandle, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || is_reparse(&metadata) || !metadata.is_dir() {
        return Err("unsafe output parent".into());
    }
    let handle = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let opened = handle.metadata().map_err(|e| e.to_string())?;
    if !opened.is_dir() || file_identity(&opened) != file_identity(&metadata) {
        return Err("output directory changed while opening".into());
    }
    Ok(OutputDirectoryHandle {
        _handle: handle,
        identity: file_identity(&opened),
    })
}
#[cfg(unix)]
fn same_output_directory(
    path: &std::path::Path,
    held: &OutputDirectoryHandle,
) -> Result<bool, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    Ok(metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && file_identity(&metadata) == held.identity)
}

#[cfg(windows)]
struct OutputDirectoryHandle {
    handle: windows_sys::Win32::Foundation::HANDLE,
    identity: FileIdentity,
}
#[cfg(windows)]
impl Drop for OutputDirectoryHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
#[cfg(windows)]
fn open_safe_output_directory(path: &std::path::Path) -> Result<OutputDirectoryHandle, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || is_reparse(&metadata) || !metadata.is_dir() {
        return Err("unsafe output parent".into());
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "cannot open output directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return Err(format!(
            "cannot identify output directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    if info.dwFileAttributes & 0x400 != 0 {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return Err("unsafe output parent reparse point".into());
    }
    Ok(OutputDirectoryHandle {
        handle,
        identity: (
            info.dwVolumeSerialNumber,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        ),
    })
}
#[cfg(windows)]
fn same_output_directory(
    path: &std::path::Path,
    held: &OutputDirectoryHandle,
) -> Result<bool, String> {
    let current = open_safe_output_directory(path)?;
    Ok(current.identity == held.identity)
}
#[cfg(unix)]
type FileIdentity = (u64, u64);
#[cfg(windows)]
type FileIdentity = (u32, u64);

fn canonical_candidate(mut candidate: Candidate) -> Candidate {
    candidate.genre_ids.sort();
    candidate.mood_ids.sort();
    candidate
        .inference
        .parameters
        .sort_by(|a, b| a.name.cmp(&b.name));
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    fn fixture() -> String {
        r#"{
  "schema": "adhd-music.candidate-ledger.planned",
  "schema_version": 1,
  "batch": {
    "id": "2026-07-batch-01",
    "created_at": "2026-07-11T00:00:00Z",
    "notes": "Planned instrumental generation batch",
    "generator_pin": {
      "config": "acestep-v15-turbo",
      "planner": "acestep-5Hz-lm-0.6B",
      "python_version": "3.12",
      "source_url": "https://github.com/ace-step/ACE-Step-1.5.git",
      "source_commit": "6d467e4b5081ccb0abf1ec1bf4fdf9051a2d34b0",
      "turbo_vae_repo": "ACE-Step/Ace-Step1.5",
      "turbo_vae_revision": "19671f406d603126926c1b7e2adc169acbcade22",
      "planner_repo": "ACE-Step/acestep-5Hz-lm-0.6B",
      "planner_revision": "148d8ea0225bdab342ee1ae3a354275ccd60ca80"
    },
    "terms_evidence": {
      "licence_id": "ace-step-1.5",
      "licence_url": "https://github.com/ace-step/ACE-Step-1.5/blob/main/LICENSE",
      "model_card_url": "https://github.com/ace-step/ACE-Step-1.5/blob/main/README.md",
      "output_licence": "ace-step-1.5-output-terms",
      "source_archive_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  },
  "taxonomy": {
    "activities": ["deep_work", "motivation", "creativity", "learning", "light_work"],
    "genres": [
      {"id": "ambient", "label": "Ambient"},
      {"id": "pulse", "label": "Pulse"}
    ],
    "moods": [
      {"id": "calm", "label": "Calm"},
      {"id": "steady", "label": "Steady"}
    ]
  },
  "candidates": [
    {
      "id": "c-001",
      "seed": 12345,
      "activity": "deep_work",
      "genre_ids": ["ambient"],
      "mood_ids": ["calm"],
      "duration_seconds": 120.0,
      "bpm": 90,
      "contains_lyrics": false,
      "contains_speech": false,
      "prompts": {
        "positive": "ambient pads, steady pulse, no vocals [Instrumental]",
        "negative": "vocals, lyrics, speech, percussion"
      },
      "inference": {
        "codec": "flac",
        "sample_rate_hz": 48000,
        "steps": 8,
        "shift": 3,
        "solver": "ode",
        "use_random_seed": false,
        "parameters": [
          {"name": "cfg_scale", "value": "3.0"},
          {"name": "guidance", "value": "low"}
        ]
      }
    },
    {
      "id": "c-002",
      "seed": 67890,
      "activity": "creativity",
      "genre_ids": ["pulse"],
      "mood_ids": ["steady"],
      "duration_seconds": 90.0,
      "bpm": 110,
      "contains_lyrics": false,
      "contains_speech": false,
      "prompts": {
        "positive": "gentle pulse for ideation [Instrumental]",
        "negative": "vocals, lyrics, speech, silence"
      },
      "inference": {
        "codec": "flac",
        "sample_rate_hz": 48000,
        "steps": 8,
        "shift": 3,
        "solver": "ode",
        "use_random_seed": false,
        "parameters": [
          {"name": "cfg_scale", "value": "3.5"}
        ]
      }
    }
  ]
}"#
        .to_owned()
    }

    fn parse_and_validate(json: &str) -> Result<PlannedBatch, String> {
        let batch: PlannedBatch = from_str(json).map_err(|error| format!("parse: {error}"))?;
        batch
            .validate()
            .map(|()| batch)
            .map_err(|error| format!("validate: {error}"))
    }

    fn assert_invalid(json: &str, needle: &str) {
        match parse_and_validate(json) {
            Ok(_) => panic!("expected rejection containing {needle:?}, but input was accepted"),
            Err(message) => assert!(
                message.contains(needle),
                "expected error containing {needle:?}, got {message:?}"
            ),
        }
    }

    #[test]
    fn valid_batch_round_trips_deterministically() {
        let batch = parse_and_validate(&fixture()).expect("fixture must be valid");

        let first = canonical_bytes(&batch).expect("first canonical");
        let reparsed: PlannedBatch =
            from_slice(&first).expect("canonical bytes must reparse strictly");
        reparsed.validate().expect("canonical batch must validate");
        let second = canonical_bytes(&reparsed).expect("second canonical");
        assert_eq!(first, second, "canonical emission must be deterministic");
        assert_eq!(batch.candidates.first().unwrap().id, "c-001");
    }

    #[test]
    fn duplicate_json_keys_are_rejected() {
        let duplicated = fixture().replace("\"seed\": 12345,", "\"seed\": 12345, \"seed\": 99999,");
        let Err(error) = from_str::<PlannedBatch>(&duplicated) else {
            panic!("duplicate keys must be rejected at parse time");
        };
        assert!(
            error.to_string().contains("duplicate JSON key"),
            "expected duplicate-key error, got {error}"
        );
    }

    #[test]
    fn duplicate_candidate_ids_are_rejected() {
        let duplicated = fixture().replace("\"id\": \"c-002\"", "\"id\": \"c-001\"");
        assert_invalid(&duplicated, "duplicate candidate id");
    }

    #[test]
    fn duplicate_seeds_are_rejected() {
        let duplicated = fixture().replace("\"seed\": 67890", "\"seed\": 12345");
        assert_invalid(&duplicated, "reuses seed");
    }

    #[test]
    fn traversal_and_unsafe_names_are_rejected() {
        for bad in [
            "../escape",
            "/abs",
            "a/b",
            "a\\b",
            "Bad ID",
            "UPPER",
            "-lead",
            "trail-",
        ] {
            let json = fixture().replace("\"id\": \"c-001\"", &format!("\"id\": \"{bad}\""));
            assert_invalid(&json, "safe planned name");
        }
    }

    #[test]
    fn pin_tampering_is_rejected() {
        let source = fixture().replace(
            "6d467e4b5081ccb0abf1ec1bf4fdf9051a2d34b0",
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );
        assert_invalid(&source, "source_commit mismatch");

        let turbo = fixture().replace(
            "19671f406d603126926c1b7e2adc169acbcade22",
            "0000000000000000000000000000000000000000",
        );
        assert_invalid(&turbo, "turbo_vae_revision mismatch");

        let config = fixture().replace("acestep-v15-turbo", "acestep-v15-base");
        assert_invalid(&config, "generator_pin.config must be");
    }

    #[test]
    fn missing_instrumental_marker_is_rejected() {
        let no_marker = fixture().replace(
            "ambient pads, steady pulse, no vocals [Instrumental]",
            "ambient pads, steady pulse, no vocals",
        );
        assert_invalid(&no_marker, "[Instrumental]");
    }

    #[test]
    fn cc0_and_unknown_qa_fields_are_rejected() {
        let cc0 = fixture().replace(
            "\"output_licence\": \"ace-step-1.5-output-terms\"",
            "\"output_licence\": \"CC0\"",
        );
        assert_invalid(&cc0, "must not claim CC0");

        let with_qa = fixture().replace(
            "\"id\": \"c-001\"",
            "\"id\": \"c-001\", \"human_qa\": {\"status\": \"approved\"}",
        );
        let Err(error) = from_str::<PlannedBatch>(&with_qa) else {
            panic!("human-QA field must be rejected at parse time");
        };
        assert!(
            error.to_string().contains("unknown field"),
            "expected unknown-field error, got {error}"
        );
    }

    #[test]
    fn invalid_inference_parameters_are_rejected() {
        let steps = fixture().replacen("\"steps\": 8,", "\"steps\": 9,", 1);
        assert_invalid(&steps, "inference steps must be");

        let shift = fixture().replacen("\"shift\": 3,", "\"shift\": 4,", 1);
        assert_invalid(&shift, "inference shift must be");

        let random =
            fixture().replacen("\"use_random_seed\": false", "\"use_random_seed\": true", 1);
        assert_invalid(&random, "use_random_seed must be false");

        let codec = fixture().replacen("\"codec\": \"flac\"", "\"codec\": \"wav\"", 1);
        assert_invalid(&codec, "inference codec must be flac");

        let rate = fixture().replacen("\"sample_rate_hz\": 48000", "\"sample_rate_hz\": 44100", 1);
        assert_invalid(&rate, "sample_rate_hz must be");

        let solver = fixture().replacen("\"solver\": \"ode\"", "\"solver\": \"euler\"", 1);
        assert_invalid(&solver, "solver must be ode");
    }

    #[test]
    fn unordered_or_duplicate_parameter_names_are_rejected() {
        let unordered = fixture().replace(
            "{\"name\": \"cfg_scale\", \"value\": \"3.0\"},\n          {\"name\": \"guidance\", \"value\": \"low\"}",
            "{\"name\": \"guidance\", \"value\": \"low\"},\n          {\"name\": \"cfg_scale\", \"value\": \"3.0\"}",
        );
        assert_invalid(&unordered, "parameters must be ordered by name");

        let duplicate = fixture().replace(
            "{\"name\": \"cfg_scale\", \"value\": \"3.0\"},\n          {\"name\": \"guidance\", \"value\": \"low\"}",
            "{\"name\": \"cfg_scale\", \"value\": \"3.0\"},\n          {\"name\": \"cfg_scale\", \"value\": \"4.0\"}",
        );
        assert_invalid(&duplicate, "repeats parameter");
    }

    #[test]
    fn noncanonical_sha256_and_generated_asset_hash_are_rejected() {
        let uppercase = fixture().replace(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
        );
        assert_invalid(&uppercase, "canonical lowercase SHA-256");

        let short = fixture().replace(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "abc",
        );
        assert_invalid(&short, "canonical lowercase SHA-256");

        let with_asset = fixture().replace(
            "\"id\": \"c-001\"",
            "\"id\": \"c-001\", \"asset_sha256\": \"deadbeef\"",
        );
        let Err(error) = from_str::<PlannedBatch>(&with_asset) else {
            panic!("generated asset hash must be rejected at parse time");
        };
        assert!(
            error.to_string().contains("unknown field"),
            "expected unknown-field error, got {error}"
        );
    }

    #[test]
    fn lyrics_speech_duration_bpm_and_activity_are_rejected() {
        let lyrics =
            fixture().replacen("\"contains_lyrics\": false", "\"contains_lyrics\": true", 1);
        assert_invalid(&lyrics, "declares lyrics");

        let speech =
            fixture().replacen("\"contains_speech\": false", "\"contains_speech\": true", 1);
        assert_invalid(&speech, "declares speech");

        let empty_negative = fixture().replace(
            "\"negative\": \"vocals, lyrics, speech, percussion\"",
            "\"negative\": \"   \"",
        );
        assert_invalid(&empty_negative, "negative prompt");

        let short_duration =
            fixture().replace("\"duration_seconds\": 120.0", "\"duration_seconds\": 5.0");
        assert_invalid(&short_duration, "duration_seconds must be within");

        let slow_bpm = fixture().replace("\"bpm\": 90", "\"bpm\": 30");
        assert_invalid(&slow_bpm, "bpm must be within");

        let unsupported_activity = fixture().replace(
            "\"activities\": [\"deep_work\", \"motivation\", \"creativity\", \"learning\", \"light_work\"]",
            "\"activities\": [\"motivation\", \"creativity\", \"learning\", \"light_work\"]",
        );
        assert_invalid(&unsupported_activity, "unsupported activity");
    }

    #[test]
    fn unknown_publication_field_is_rejected() {
        let with_unknown = fixture().replace(
            "\"created_at\": \"2026-07-11T00:00:00Z\"",
            "\"created_at\": \"2026-07-11T00:00:00Z\", \"publication\": {\"id\": \"x\"}",
        );
        let Err(error) = from_str::<PlannedBatch>(&with_unknown) else {
            panic!("publication field must be rejected at parse time");
        };
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn capture_rejects_file_changed_after_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.json");
        std::fs::write(&path, b"original").unwrap();
        let error = capture_inner(&path, 1024, || {
            std::fs::write(&path, b"replacement-is-longer").unwrap()
        })
        .expect_err("an opened handle whose length changes must be rejected");
        assert!(error.contains("changed"), "{error}");
    }

    #[test]
    fn output_directory_handle_identity_is_stable_for_a_live_directory() {
        let dir = tempfile::tempdir().unwrap();
        let held = open_safe_output_directory(dir.path()).unwrap();
        assert!(same_output_directory(dir.path(), &held).unwrap());
    }

    #[test]
    fn generated_record_is_canonical_and_records_validated_evidence() {
        let fixture_bytes = include_bytes!("../tests/fixtures/sine-220hz-10s-48khz-stereo.flac");
        let hash = format!("{:x}", sha2::Sha256::digest(fixture_bytes));
        let report = format!(
            r#"{{"source":{{"file_name":"asset.FLAC","bytes":{},"sha256":"{}"}},"decode":{{"status":"decoded","codec":"flac","sample_rate_hz":48000,"channels":2,"frames":480000,"duration_seconds":10.0}},"hard_rejections":[]}}"#,
            fixture_bytes.len(),
            hash
        );
        let report_hash = format!("{:x}", sha2::Sha256::digest(report.as_bytes()));
        let evidence = format!(
            r#"{{"schema":"adhd-music.candidate-ledger.generation-evidence","schema_version":1,"candidate_id":"c-001","generated_at":"2026-07-11T00:00:00Z","machine":"test-machine","gpu":"test-gpu","output":{{"file_name":"asset.FLAC","bytes":{},"codec":"flac","sample_rate_hz":48000,"channels":2,"sha256":"{}"}},"analyzer":{{"file_name":"analysis.json","sha256":"{}"}},"evidence_file_name":"evidence.json","edit_lineage":[]}}"#,
            fixture_bytes.len(),
            hash,
            report_hash
        );
        let make_record = |reordered: bool| {
            let dir = tempfile::tempdir().unwrap();
            let plan = if reordered {
                fixture()
                    .replace(
                        "\"genre_ids\": [\"ambient\"]",
                        "\"genre_ids\": [\"pulse\", \"ambient\"]",
                    )
                    .replace(
                        "\"mood_ids\": [\"calm\"]",
                        "\"mood_ids\": [\"steady\", \"calm\"]",
                    )
            } else {
                fixture()
                    .replace(
                        "\"genre_ids\": [\"ambient\"]",
                        "\"genre_ids\": [\"ambient\", \"pulse\"]",
                    )
                    .replace(
                        "\"mood_ids\": [\"calm\"]",
                        "\"mood_ids\": [\"calm\", \"steady\"]",
                    )
            }
            .replace("\"duration_seconds\": 120.0", "\"duration_seconds\": 10.0");
            std::fs::write(dir.path().join("plan.json"), plan).unwrap();
            std::fs::write(dir.path().join("asset.FLAC"), fixture_bytes).unwrap();
            std::fs::write(dir.path().join("analysis.json"), &report).unwrap();
            std::fs::write(dir.path().join("evidence.json"), &evidence).unwrap();
            let args = [
                ("--plan", "plan.json"),
                ("--candidate", "c-001"),
                ("--asset", "asset.FLAC"),
                ("--analysis", "analysis.json"),
                ("--evidence", "evidence.json"),
                ("--output", "record.json"),
            ]
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_owned(),
                    if k == "--candidate" {
                        std::path::PathBuf::from(v)
                    } else {
                        dir.path().join(v)
                    },
                )
            })
            .collect();
            register(&args).unwrap();
            std::fs::read(dir.path().join("record.json")).unwrap()
        };
        let first = make_record(false);
        let second = make_record(true);
        assert_eq!(
            first, second,
            "order-only valid inputs must serialize identically"
        );
        let record: GeneratedRecord = from_slice(&first).unwrap();
        assert_eq!(record.evidence.generated_at, "2026-07-11T00:00:00Z");
        assert_eq!(record.evidence.machine, "test-machine");
        assert_eq!(record.evidence.gpu, "test-gpu");
        assert_eq!(record.candidate.genre_ids, ["ambient", "pulse"]);
        assert_eq!(record.candidate.mood_ids, ["calm", "steady"]);
    }

    fn valid_registration_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let asset = include_bytes!("../tests/fixtures/sine-220hz-10s-48khz-stereo.flac");
        let hash = format!("{:x}", sha2::Sha256::digest(asset));
        let plan = fixture().replace("\"duration_seconds\": 120.0", "\"duration_seconds\": 10.0");
        let report = format!(
            r#"{{"source":{{"file_name":"asset.flac","bytes":{},"sha256":"{}"}},"decode":{{"status":"decoded","codec":"flac","sample_rate_hz":48000,"channels":2,"frames":480000,"duration_seconds":10.0}},"hard_rejections":[]}}"#,
            asset.len(),
            hash
        );
        let report_hash = format!("{:x}", sha2::Sha256::digest(report.as_bytes()));
        let evidence = format!(
            r#"{{"schema":"adhd-music.candidate-ledger.generation-evidence","schema_version":1,"candidate_id":"c-001","generated_at":"2026-07-11T00:00:00Z","machine":"test-machine","gpu":"test-gpu","output":{{"file_name":"asset.flac","bytes":{},"codec":"flac","sample_rate_hz":48000,"channels":2,"sha256":"{}"}},"analyzer":{{"file_name":"analysis.json","sha256":"{}"}},"evidence_file_name":"evidence.json","edit_lineage":[]}}"#,
            asset.len(),
            hash,
            report_hash
        );
        std::fs::write(dir.path().join("plan.json"), plan).unwrap();
        std::fs::write(dir.path().join("asset.flac"), asset).unwrap();
        std::fs::write(dir.path().join("analysis.json"), report).unwrap();
        std::fs::write(dir.path().join("evidence.json"), evidence).unwrap();
        dir
    }
    fn run_registration(dir: &tempfile::TempDir, extra: &[&str]) -> Result<(), String> {
        let mut args: Vec<std::ffi::OsString> =
            vec!["candidate-ledger".into(), "register-generated".into()];
        for (key, value) in [
            ("--plan", "plan.json"),
            ("--candidate", "c-001"),
            ("--asset", "asset.flac"),
            ("--analysis", "analysis.json"),
            ("--evidence", "evidence.json"),
            ("--output", "record.json"),
        ] {
            args.push(key.into());
            args.push(if key == "--candidate" {
                value.into()
            } else {
                dir.path().join(value).into_os_string()
            });
        }
        args.extend(extra.iter().map(|s| (*s).into()));
        run(args)
    }
    fn assert_registration_rejected(label: &str, mutate: impl FnOnce(&std::path::Path)) {
        let dir = valid_registration_dir();
        mutate(dir.path());
        assert!(run_registration(&dir, &[]).is_err(), "{label} accepted");
        assert!(
            !dir.path().join("record.json").exists(),
            "{label} wrote output"
        );
    }
    fn replace_asset_facts(dir: &std::path::Path, source: &std::path::Path) {
        std::fs::copy(source, dir.join("asset.flac")).unwrap();
        let asset = capture(&dir.join("asset.flac"), MAX_ASSET_BYTES).unwrap();
        let decoded = decode_flac(&asset.bytes).ok();
        let (rate, channels, frames, duration) = decoded
            .as_ref()
            .map(|d| (d.sample_rate, d.channels, d.frames, d.duration))
            .unwrap_or((48_000, 2, 480_000, 10.0));
        let report = format!(
            r#"{{"source":{{"file_name":"asset.flac","bytes":{},"sha256":"{}"}},"decode":{{"status":"decoded","codec":"flac","sample_rate_hz":{},"channels":{},"frames":{},"duration_seconds":{}}},"hard_rejections":[]}}"#,
            asset.bytes.len(),
            asset.sha256,
            rate,
            channels,
            frames,
            duration
        );
        let rh = format!("{:x}", sha2::Sha256::digest(report.as_bytes()));
        let evidence = format!(
            r#"{{"schema":"adhd-music.candidate-ledger.generation-evidence","schema_version":1,"candidate_id":"c-001","generated_at":"2026-07-11T00:00:00Z","machine":"test-machine","gpu":"test-gpu","output":{{"file_name":"asset.flac","bytes":{},"codec":"flac","sample_rate_hz":{},"channels":{},"sha256":"{}"}},"analyzer":{{"file_name":"analysis.json","sha256":"{}"}},"evidence_file_name":"evidence.json","edit_lineage":[]}}"#,
            asset.bytes.len(),
            rate,
            channels,
            asset.sha256,
            rh
        );
        std::fs::write(dir.join("analysis.json"), report).unwrap();
        std::fs::write(dir.join("evidence.json"), evidence).unwrap();
    }

    #[test]
    fn registration_argument_matrix_executes_real_cli() {
        let dir = valid_registration_dir();
        for (label, args) in [
            (
                "missing-pair",
                vec!["candidate-ledger", "register-generated", "--plan"],
            ),
            (
                "repeated",
                vec![
                    "candidate-ledger",
                    "validate-plan",
                    "--plan",
                    "x",
                    "--plan",
                    "x",
                ],
            ),
            (
                "unknown",
                vec!["candidate-ledger", "validate-plan", "--nope", "x"],
            ),
            (
                "positional",
                vec!["candidate-ledger", "validate-plan", "oops", "x"],
            ),
            (
                "missing-value",
                vec!["candidate-ledger", "validate-plan", "--plan", "-bad"],
            ),
        ] {
            assert!(run(args).is_err(), "{label} accepted");
        }
        let args = vec![
            "candidate-ledger".into(),
            "register-generated".into(),
            "--output".into(),
            dir.path().join("record.json").into_os_string(),
            "--candidate".into(),
            "c-001".into(),
            "--evidence".into(),
            dir.path().join("evidence.json").into_os_string(),
            "--analysis".into(),
            dir.path().join("analysis.json").into_os_string(),
            "--asset".into(),
            dir.path().join("asset.flac").into_os_string(),
            "--plan".into(),
            dir.path().join("plan.json").into_os_string(),
        ];
        run(args).unwrap();
    }

    #[test]
    fn registration_evidence_schema_matrix() {
        for (label, needle, replacement) in [
            (
                "duplicate-root",
                "\"gpu\":\"test-gpu\"",
                "\"gpu\":\"test-gpu\",\"gpu\":\"x\"",
            ),
            (
                "duplicate-nested",
                "\"channels\":2",
                "\"channels\":2,\"channels\":2",
            ),
            (
                "nested-unknown",
                "\"channels\":2",
                "\"channels\":2,\"unknown\":true",
            ),
            (
                "cc0",
                "\"schema_version\":1",
                "\"schema_version\":1,\"CC0\":true",
            ),
            (
                "human-qa",
                "\"machine\":\"test-machine\"",
                "\"machine\":\"test-machine\",\"human_qa\":true",
            ),
            (
                "publication",
                "\"gpu\":\"test-gpu\"",
                "\"gpu\":\"test-gpu\",\"publication\":true",
            ),
            (
                "non-utc",
                "2026-07-11T00:00:00Z",
                "2026-07-11T01:00:00+01:00",
            ),
            ("invalid-time", "2026-07-11T00:00:00Z", "not-a-time"),
            ("empty-machine", "test-machine", ""),
            ("empty-gpu", "test-gpu", ""),
            (
                "lineage",
                "\"edit_lineage\":[]",
                "\"edit_lineage\":[\"edit\"]",
            ),
        ] {
            assert_registration_rejected(label, |p| {
                let s = std::fs::read_to_string(p.join("evidence.json"))
                    .unwrap()
                    .replacen(needle, replacement, 1);
                std::fs::write(p.join("evidence.json"), s).unwrap();
            });
        }
    }

    #[test]
    fn registration_evidence_fact_matrix() {
        for (label, needle, replacement) in [
            ("bytes", "\"bytes\":179532", "\"bytes\":1"),
            ("asset-hash", "\"sha256\":\"d9ed", "\"sha256\":\"a9ed"),
            ("codec", "\"codec\":\"flac\"", "\"codec\":\"wav\""),
            ("rate", "\"sample_rate_hz\":48000", "\"sample_rate_hz\":1"),
            ("channels", "\"channels\":2", "\"channels\":1"),
            ("basename", "asset.flac", "CON.flac.json"),
        ] {
            assert_registration_rejected(label, |p| {
                let s = std::fs::read_to_string(p.join("evidence.json"))
                    .unwrap()
                    .replacen(needle, replacement, 1);
                std::fs::write(p.join("evidence.json"), s).unwrap();
            });
        }
    }

    #[test]
    fn registration_asset_and_analyzer_matrix() {
        assert_registration_rejected("corrupt-asset", |p| {
            std::fs::write(p.join("asset.flac"), b"not flac").unwrap()
        });
        assert_registration_rejected("substituted-asset", |p| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(p.join("asset.flac"))
                .unwrap()
                .write_all(b"x")
                .unwrap();
        });
        for (label, needle, replacement) in [
            (
                "hard-rejection",
                "\"hard_rejections\":[]",
                "\"hard_rejections\":[\"x\"]",
            ),
            ("source-name", "asset.flac", "other.flac"),
            ("source-bytes", "\"bytes\":179532", "\"bytes\":1"),
            (
                "decode-status",
                "\"status\":\"decoded\"",
                "\"status\":\"failed\"",
            ),
            ("decode-codec", "\"codec\":\"flac\"", "\"codec\":\"wav\""),
            (
                "decode-rate",
                "\"sample_rate_hz\":48000",
                "\"sample_rate_hz\":1",
            ),
            ("decode-channels", "\"channels\":2", "\"channels\":1"),
            ("decode-frames", "\"frames\":480000", "\"frames\":1"),
            (
                "decode-duration",
                "\"duration_seconds\":10.0",
                "\"duration_seconds\":1.0",
            ),
        ] {
            assert_registration_rejected(label, |p| {
                let s = std::fs::read_to_string(p.join("analysis.json"))
                    .unwrap()
                    .replacen(needle, replacement, 1);
                std::fs::write(p.join("analysis.json"), s).unwrap();
            });
        }
    }

    #[test]
    fn registration_publication_matrix() {
        let dir = valid_registration_dir();
        run_registration(&dir, &[]).unwrap();
        let before = std::fs::read(dir.path().join("record.json")).unwrap();
        assert!(run_registration(&dir, &[]).is_err());
        assert_eq!(
            before,
            std::fs::read(dir.path().join("record.json")).unwrap()
        );
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tmp")));
    }

    #[test]
    fn registration_unsafe_asset_basename_matrix() {
        for name in [
            "CON.flac.json",
            "bad?.flac",
            "..\\asset.flac",
            "asset.flac. ",
        ] {
            assert_registration_rejected(name, |p| {
                let s = std::fs::read_to_string(p.join("evidence.json"))
                    .unwrap()
                    .replace("asset.flac", name);
                std::fs::write(p.join("evidence.json"), s).unwrap();
            });
        }
    }
    #[test]
    fn registration_analyzer_and_evidence_name_matrix() {
        for name in [
            "CON.json.txt",
            "bad?.json",
            "..\\analysis.json",
            "analysis.json. ",
        ] {
            assert_registration_rejected(name, |p| {
                let s = std::fs::read_to_string(p.join("evidence.json"))
                    .unwrap()
                    .replace("analysis.json", name);
                std::fs::write(p.join("evidence.json"), s).unwrap();
            });
        }
    }
    #[test]
    fn registration_output_aliases_each_input_are_rejected() {
        for input in ["plan.json", "asset.flac", "analysis.json", "evidence.json"] {
            let dir = valid_registration_dir();
            let mut args: Vec<std::ffi::OsString> =
                vec!["candidate-ledger".into(), "register-generated".into()];
            for (k, v) in [
                ("--plan", "plan.json"),
                ("--candidate", "c-001"),
                ("--asset", "asset.flac"),
                ("--analysis", "analysis.json"),
                ("--evidence", "evidence.json"),
                ("--output", input),
            ] {
                args.push(k.into());
                args.push(if k == "--candidate" {
                    v.into()
                } else {
                    dir.path().join(v).into_os_string()
                });
            }
            assert!(run(args).is_err(), "{input} alias accepted");
        }
    }
    #[test]
    fn registration_analyzer_unknown_and_duplicate_keys_reject() {
        for (label, suffix) in [
            ("unknown", ",\"unknown\":true"),
            ("duplicate", ",\"source\":{}"),
        ] {
            assert_registration_rejected(label, |p| {
                let mut s = std::fs::read_to_string(p.join("analysis.json")).unwrap();
                s.pop();
                s.push_str(suffix);
                s.push('}');
                std::fs::write(p.join("analysis.json"), s).unwrap();
            });
        }
    }

    fn assert_audio_fixture_rejected(label: &str, source: &str, reason: &str) {
        let dir = valid_registration_dir();
        replace_asset_facts(dir.path(), std::path::Path::new(source));
        let error = run_registration(&dir, &[]).expect_err(label);
        assert!(error.contains(reason), "{label}: {error}");
        assert!(!dir.path().join("record.json").exists());
    }
    #[test]
    fn registration_rejects_wav_bytes_named_flac() {
        assert_audio_fixture_rejected(
            "wav-as-flac",
            "../../crates/audio-engine/tests/fixtures/wav_pcm24_stereo_48000.wav",
            "FLAC",
        );
    }
    #[test]
    fn registration_rejects_44100_stereo_flac() {
        assert_audio_fixture_rejected(
            "44100-stereo",
            "tests/fixtures/sine-220hz-10s-44100hz-stereo.flac",
            "48 kHz stereo",
        );
    }
    #[test]
    fn registration_rejects_48000_mono_flac() {
        assert_audio_fixture_rejected(
            "48000-mono",
            "tests/fixtures/sine-220hz-10s-48khz-mono.flac",
            "48 kHz stereo",
        );
    }
    #[test]
    fn registration_rejects_one_second_duration_mismatch() {
        assert_audio_fixture_rejected(
            "one-second-duration",
            "../../crates/audio-engine/tests/fixtures/flac_stereo_48000.flac",
            "decoded duration does not match",
        );
    }
}
