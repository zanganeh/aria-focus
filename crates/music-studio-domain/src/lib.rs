//! Deterministic, local domain types for Music Studio.
//!
//! This crate validates requests and models jobs. It deliberately does not run
//! generation, inspect audio, launch processes, or depend on desktop runtime
//! services.

use std::fmt;

use domain::Activity;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub const LOCKED_NEGATIVE_PROMPT: &str =
    "vocals, lyrics, spoken words, narration, speech, voice, chanting, rap, dialogue";
const MAX_DETAILS_CHARS: usize = 500;
const MAX_CREATIVE_PROMPT_CHARS: usize = 2_000;
const MAX_FAILURE_DETAILS_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StudioEnergy {
    Low,
    #[default]
    Medium,
    High,
}

/// The only durations supported by the local Music Studio request contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StudioDuration {
    Seconds90,
    #[default]
    Seconds180,
}

impl StudioDuration {
    pub const fn seconds(self) -> u16 {
        match self {
            Self::Seconds90 => 90,
            Self::Seconds180 => 180,
        }
    }
}

impl Serialize for StudioDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.seconds())
    }
}

impl<'de> Deserialize<'de> for StudioDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u16::deserialize(deserializer)? {
            90 => Ok(Self::Seconds90),
            180 => Ok(Self::Seconds180),
            _ => Err(de::Error::custom(
                "duration must be exactly 90 or 180 seconds",
            )),
        }
    }
}

/// A stable local catalogue identifier, not free-form display text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StudioId(String);

impl StudioId {
    pub fn new(value: impl Into<String>) -> Result<Self, StudioError> {
        let value = value.into();
        if !is_stable_id(&value) {
            return Err(StudioError::new(
                StudioErrorCode::InvalidId,
                "invalid stable identifier",
            ));
        }
        if contains_forbidden_wording(&value) {
            return Err(StudioError::new(
                StudioErrorCode::ForbiddenWording,
                "identifier contains forbidden wording",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for StudioId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StudioId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Validated input used to build one local Music Studio request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StudioPromptInput {
    pub activity: Activity,
    pub genre_id: StudioId,
    pub mood_id: Option<StudioId>,
    pub energy: StudioEnergy,
    pub instrument_ids: Vec<StudioId>,
    pub additional_details: Option<String>,
    pub edited_creative_prompt: Option<String>,
    pub duration: StudioDuration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioPromptInputWire {
    activity: Activity,
    genre_id: StudioId,
    #[serde(default)]
    mood_id: Option<StudioId>,
    #[serde(default)]
    energy: StudioEnergy,
    #[serde(default)]
    instrument_ids: Vec<StudioId>,
    #[serde(default)]
    additional_details: Option<String>,
    #[serde(default)]
    edited_creative_prompt: Option<String>,
    #[serde(default)]
    duration: StudioDuration,
}

impl<'de> Deserialize<'de> for StudioPromptInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StudioPromptInputWire::deserialize(deserializer)?;
        Self::new(
            wire.activity,
            wire.genre_id,
            wire.mood_id,
            wire.energy,
            wire.instrument_ids,
            wire.additional_details,
            wire.edited_creative_prompt,
            wire.duration,
        )
        .map_err(de::Error::custom)
    }
}

impl StudioPromptInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        activity: Activity,
        genre_id: StudioId,
        mood_id: Option<StudioId>,
        energy: StudioEnergy,
        instrument_ids: Vec<StudioId>,
        additional_details: Option<String>,
        edited_creative_prompt: Option<String>,
        duration: StudioDuration,
    ) -> Result<Self, StudioError> {
        if instrument_ids.len() > 5 {
            return Err(StudioError::new(
                StudioErrorCode::InvalidRequest,
                "at most five instruments are allowed",
            ));
        }
        validate_optional_text(
            additional_details.as_deref(),
            MAX_DETAILS_CHARS,
            "additional details",
        )?;
        validate_optional_text(
            edited_creative_prompt.as_deref(),
            MAX_CREATIVE_PROMPT_CHARS,
            "edited creative prompt",
        )?;
        Ok(Self {
            activity,
            genre_id,
            mood_id,
            energy,
            instrument_ids,
            additional_details,
            edited_creative_prompt,
            duration,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuiltStudioPrompt {
    pub template_version: u16,
    pub creative_prompt: String,
    pub locked_negative_prompt: String,
    pub duration_seconds: u16,
    pub seed: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuiltStudioPromptWire {
    template_version: u16,
    creative_prompt: String,
    locked_negative_prompt: String,
    duration_seconds: u16,
    seed: u64,
}

impl<'de> Deserialize<'de> for BuiltStudioPrompt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = BuiltStudioPromptWire::deserialize(deserializer)?;
        let prompt = Self {
            template_version: wire.template_version,
            creative_prompt: wire.creative_prompt,
            locked_negative_prompt: wire.locked_negative_prompt,
            duration_seconds: wire.duration_seconds,
            seed: wire.seed,
        };
        validate_built_prompt(&prompt).map_err(de::Error::custom)?;
        Ok(prompt)
    }
}

/// Builds the local template without invoking an AI rewrite or any runtime service.
pub fn build_studio_prompt(
    input: &StudioPromptInput,
    seed: u64,
) -> Result<BuiltStudioPrompt, StudioError> {
    let input = StudioPromptInput::new(
        input.activity,
        input.genre_id.clone(),
        input.mood_id.clone(),
        input.energy,
        input.instrument_ids.clone(),
        input.additional_details.clone(),
        input.edited_creative_prompt.clone(),
        input.duration,
    )?;
    let mut parts = vec![
        "instrumental music only".to_owned(),
        format!("activity: {}", input.activity.storage_key()),
        format!("genre: {}", input.genre_id.as_str()),
        format!("energy: {}", energy_name(input.energy)),
    ];
    if let Some(mood) = input.mood_id {
        parts.push(format!("mood: {}", mood.as_str()));
    }
    if !input.instrument_ids.is_empty() {
        parts.push(format!(
            "instruments: {}",
            input
                .instrument_ids
                .iter()
                .map(StudioId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(details) = input.additional_details {
        parts.push(format!("details: {details}"));
    }
    if let Some(edited) = input.edited_creative_prompt {
        parts.push(format!("creative direction: {edited}"));
    }
    Ok(BuiltStudioPrompt {
        template_version: 1,
        creative_prompt: parts.join("; "),
        locked_negative_prompt: LOCKED_NEGATIVE_PROMPT.to_owned(),
        duration_seconds: input.duration.seconds(),
        seed,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Checking,
    Ready,
    SetupRequired,
    Unsupported,
    NeedsAttention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioRuntimeInfo {
    pub present: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioHardwareInfo {
    pub architecture: Option<String>,
    pub memory_bytes: Option<u64>,
    pub accelerator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda: Option<bool>,
}

/// Minimum hardware/environment requirements the Music Studio needs to run.
///
/// These are reported to the UI so users can see exactly what the packaged
/// runtime needs before they start a one-time setup. They are advisory values
/// used by the preflight check, not a guarantee that generation will succeed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioRequirements {
    pub architecture: String,
    pub min_memory_bytes: u64,
    pub min_vram_bytes: u64,
    pub cuda_required: bool,
    pub min_free_disk_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioCapability {
    pub state: CapabilityState,
    pub runtime: StudioRuntimeInfo,
    pub hardware: StudioHardwareInfo,
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<StudioRequirements>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StudioJobState {
    Queued,
    Generating,
    Analyzing,
    Ready,
    Rejected,
    Failed,
    Cancelled,
    Interrupted,
    Saving,
    Saved,
}

impl StudioJobState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Failed | Self::Cancelled | Self::Interrupted | Self::Saved
        )
    }
    pub const fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Queued,
                Self::Generating | Self::Cancelled | Self::Failed | Self::Interrupted
            ) | (
                Self::Generating,
                Self::Analyzing | Self::Cancelled | Self::Failed | Self::Interrupted
            ) | (
                Self::Analyzing,
                Self::Ready | Self::Rejected | Self::Failed | Self::Interrupted
            ) | (Self::Ready, Self::Saving | Self::Cancelled)
                | (Self::Saving, Self::Saved | Self::Ready | Self::Failed)
        )
    }
}

macro_rules! opaque_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, StudioError> {
                let value = value.into();
                if value.starts_with($prefix)
                    && value.len() >= $prefix.len() + 12
                    && value.len() <= 80
                    && value
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    Ok(Self(value))
                } else {
                    Err(StudioError::new(
                        StudioErrorCode::InvalidId,
                        "malformed opaque identifier",
                    ))
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(serializer)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}
opaque_id!(StudioJobId, "job_");
opaque_id!(StudioAttemptId, "attempt_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StudioFailureDetails {
    pub code: StudioErrorCode,
    pub detail: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioFailureDetailsWire {
    code: StudioErrorCode,
    detail: String,
}

impl<'de> Deserialize<'de> for StudioFailureDetails {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StudioFailureDetailsWire::deserialize(deserializer)?;
        Self::new(wire.code, wire.detail).map_err(de::Error::custom)
    }
}
impl StudioFailureDetails {
    pub fn new(code: StudioErrorCode, detail: String) -> Result<Self, StudioError> {
        validate_text(
            &detail,
            MAX_FAILURE_DETAILS_CHARS,
            StudioErrorCode::InvalidRequest,
            "failure detail",
        )?;
        Ok(Self { code, detail })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StudioJobRecord {
    pub job_id: StudioJobId,
    pub attempt_id: StudioAttemptId,
    pub request: StudioPromptInput,
    pub prompt: BuiltStudioPrompt,
    pub state: StudioJobState,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub failure: Option<StudioFailureDetails>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioJobRecordWire {
    job_id: StudioJobId,
    attempt_id: StudioAttemptId,
    request: StudioPromptInput,
    prompt: BuiltStudioPrompt,
    state: StudioJobState,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    failure: Option<StudioFailureDetails>,
}

impl<'de> Deserialize<'de> for StudioJobRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StudioJobRecordWire::deserialize(deserializer)?;
        if wire.updated_at_ms < wire.created_at_ms {
            return Err(de::Error::custom("timestamp cannot move backwards"));
        }
        if wire.prompt.duration_seconds != wire.request.duration.seconds() {
            return Err(de::Error::custom("prompt duration does not match request"));
        }
        let expected_prompt =
            build_studio_prompt(&wire.request, wire.prompt.seed).map_err(de::Error::custom)?;
        if wire.prompt != expected_prompt {
            return Err(de::Error::custom(
                "prompt does not match the structured local template",
            ));
        }
        Ok(Self {
            job_id: wire.job_id,
            attempt_id: wire.attempt_id,
            request: wire.request,
            prompt: wire.prompt,
            state: wire.state,
            revision: wire.revision,
            created_at_ms: wire.created_at_ms,
            updated_at_ms: wire.updated_at_ms,
            failure: wire.failure,
        })
    }
}

impl StudioJobRecord {
    pub fn new(
        job_id: StudioJobId,
        attempt_id: StudioAttemptId,
        request: StudioPromptInput,
        prompt: BuiltStudioPrompt,
        created_at_ms: u64,
    ) -> Result<Self, StudioError> {
        if prompt.duration_seconds != request.duration.seconds() {
            return Err(StudioError::new(
                StudioErrorCode::InvalidRequest,
                "prompt duration does not match request",
            ));
        }
        validate_built_prompt(&prompt)?;
        Ok(Self {
            job_id,
            attempt_id,
            request,
            prompt,
            state: StudioJobState::Queued,
            revision: 0,
            created_at_ms,
            updated_at_ms: created_at_ms,
            failure: None,
        })
    }
    pub fn transition(
        &mut self,
        expected_revision: u64,
        next: StudioJobState,
        updated_at_ms: u64,
        failure: Option<StudioFailureDetails>,
    ) -> Result<(), StudioError> {
        if self.revision != expected_revision {
            return Err(StudioError::new(
                StudioErrorCode::StaleRevision,
                "job revision is stale",
            ));
        }
        if !self.state.allows(next) {
            return Err(StudioError::new(
                StudioErrorCode::InvalidTransition,
                "job state transition is not allowed",
            ));
        }
        if updated_at_ms < self.updated_at_ms {
            return Err(StudioError::new(
                StudioErrorCode::InvalidRequest,
                "timestamp cannot move backwards",
            ));
        }
        self.state = next;
        self.revision += 1;
        self.updated_at_ms = updated_at_ms;
        self.failure = failure;
        Ok(())
    }
    pub fn retry_as(
        &self,
        job_id: StudioJobId,
        attempt_id: StudioAttemptId,
        created_at_ms: u64,
    ) -> Result<Self, StudioError> {
        if !self.state.is_terminal() {
            return Err(StudioError::new(
                StudioErrorCode::InvalidTransition,
                "only a terminal job can be retried",
            ));
        }
        if job_id == self.job_id || attempt_id == self.attempt_id {
            return Err(StudioError::new(
                StudioErrorCode::InvalidId,
                "retry requires new job and attempt identifiers",
            ));
        }
        Self::new(
            job_id,
            attempt_id,
            self.request.clone(),
            self.prompt.clone(),
            created_at_ms,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioErrorCode {
    InvalidId,
    InvalidRequest,
    ForbiddenWording,
    InvalidTransition,
    StaleRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct StudioError {
    pub code: StudioErrorCode,
    pub message: &'static str,
}
impl StudioError {
    const fn new(code: StudioErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

fn is_stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !value.starts_with(['-', '_'])
        && !value.ends_with(['-', '_'])
}
fn validate_optional_text(
    value: Option<&str>,
    max: usize,
    label: &'static str,
) -> Result<(), StudioError> {
    if let Some(value) = value {
        validate_text(value, max, StudioErrorCode::InvalidRequest, label)?;
        if contains_forbidden_wording(value) {
            return Err(StudioError::new(
                StudioErrorCode::ForbiddenWording,
                "text contains forbidden wording",
            ));
        }
    }
    Ok(())
}
fn validate_text(
    value: &str,
    max: usize,
    code: StudioErrorCode,
    _label: &'static str,
) -> Result<(), StudioError> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(char::is_control)
    {
        Err(StudioError::new(
            code,
            "text is blank, too long, or contains control characters",
        ))
    } else {
        Ok(())
    }
}
fn contains_forbidden_wording(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    let words: Vec<_> = normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    words.iter().any(|word| {
        matches!(
            *word,
            "vocal"
                | "vocals"
                | "lyric"
                | "lyrics"
                | "narration"
                | "speech"
                | "voice"
                | "chanting"
                | "chant"
                | "rap"
                | "dialogue"
                | "voiceover"
                | "spokenword"
        )
    }) || words
        .windows(2)
        .any(|pair| matches!(pair, ["spoken", "words"] | ["spoken", "word"]))
}
fn energy_name(energy: StudioEnergy) -> &'static str {
    match energy {
        StudioEnergy::Low => "low",
        StudioEnergy::Medium => "medium",
        StudioEnergy::High => "high",
    }
}

fn validate_built_prompt(prompt: &BuiltStudioPrompt) -> Result<(), StudioError> {
    if prompt.template_version != 1
        || !matches!(prompt.duration_seconds, 90 | 180)
        || prompt.locked_negative_prompt != LOCKED_NEGATIVE_PROMPT
        || !prompt.creative_prompt.contains("instrumental")
    {
        return Err(StudioError::new(
            StudioErrorCode::InvalidRequest,
            "prompt does not use the locked instrumental template",
        ));
    }
    Ok(())
}

impl fmt::Display for StudioErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::InvalidId => "invalid_id",
                Self::InvalidRequest => "invalid_request",
                Self::ForbiddenWording => "forbidden_wording",
                Self::InvalidTransition => "invalid_transition",
                Self::StaleRevision => "stale_revision",
            }
        )
    }
}

#[cfg(test)]
mod tests;
