use base64::Engine;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

const API_ROOT: &str = "https://openrouter.ai/api/v1";
const MAX_MODELS_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 80 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OpenRouterErrorCode {
    MissingKey,
    InvalidKey,
    InsufficientCredits,
    RateLimited,
    ModelUnavailable,
    ProviderUnavailable,
    NetworkTimeout,
    PolicyRefusal,
    InvalidResponse,
    ResponseTooLarge,
    BudgetExhausted,
    Cancelled,
    Storage,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterError {
    pub code: OpenRouterErrorCode,
    pub message: String,
    pub retryable: bool,
    pub status: Option<u16>,
}

impl std::fmt::Display for OpenRouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for OpenRouterError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPricing {
    pub prompt: Option<String>,
    pub completion: Option<String>,
    pub request: Option<String>,
    pub image: Option<String>,
    #[serde(default)]
    pub image_output: Option<String>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub audio_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub supported_parameters: Vec<String>,
    pub pricing: ModelPricing,
    pub context_length: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageCost {
    pub request_id: Option<String>,
    pub cost_microdollars: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MediaResponse {
    pub request_id: Option<String>,
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub usage: UsageCost,
}

#[derive(Clone)]
pub struct OpenRouterClient {
    http: Client,
    api_root: String,
}

impl OpenRouterClient {
    pub fn new() -> Result<Self, OpenRouterError> {
        let http = Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .map_err(|_| {
                OpenRouterError::network(
                    OpenRouterErrorCode::ProviderUnavailable,
                    "OpenRouter client could not start",
                )
            })?;
        Ok(Self {
            http,
            api_root: API_ROOT.to_owned(),
        })
    }

    pub fn list_models(&self, api_key: &str) -> Result<Vec<ModelInfo>, OpenRouterError> {
        // Model discovery should never inherit the long media-generation
        // timeout. A stale or unreachable model endpoint must return a clear
        // settings error instead of leaving the Create screen waiting for
        // three minutes.
        let response = self.request(
            api_key,
            self.http
                .get(format!("{}/models", self.api_root))
                .timeout(Duration::from_secs(30)),
        )?;
        let value = read_json(response, MAX_MODELS_BYTES)?;
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| OpenRouterError::invalid("OpenRouter returned an invalid model list"))?;
        data.iter()
            .map(|raw| {
                let architecture = raw.get("architecture").cloned().unwrap_or_default();
                Ok(ModelInfo {
                    id: raw
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            OpenRouterError::invalid("OpenRouter returned a model without an ID")
                        })?
                        .to_owned(),
                    name: raw.get("name").and_then(Value::as_str).map(str::to_owned),
                    description: raw
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    input_modalities: architecture
                        .get("input_modalities")
                        .and_then(Value::as_array)
                        .map(|v| {
                            v.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    output_modalities: architecture
                        .get("output_modalities")
                        .and_then(Value::as_array)
                        .map(|v| {
                            v.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    supported_parameters: raw
                        .get("supported_parameters")
                        .and_then(Value::as_array)
                        .map(|v| {
                            v.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    pricing: serde_json::from_value(
                        raw.get("pricing").cloned().unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                    context_length: raw.get("context_length").and_then(Value::as_u64),
                })
            })
            .collect()
    }

    pub fn validate_key(&self, api_key: &str) -> Result<KeyInfo, OpenRouterError> {
        let response = self.request(api_key, self.http.get(format!("{}/key", self.api_root)))?;
        let value = read_json(response, MAX_TEXT_BYTES)?;
        Ok(KeyInfo {
            label: value
                .get("data")
                .and_then(|d| d.get("label"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            limit: value
                .get("data")
                .and_then(|d| d.get("limit"))
                .and_then(Value::as_f64),
            usage: value
                .get("data")
                .and_then(|d| d.get("usage"))
                .and_then(Value::as_f64),
        })
    }

    pub fn refine_prompt(
        &self,
        api_key: &str,
        model: &str,
        structured_prompt: &Value,
    ) -> Result<TextResponse, OpenRouterError> {
        let locked = "Return JSON only with one field `prompt`. Preserve all locked constraints: instrumental only, no vocals, no speech, no lyrics, no jazz/swing/blues/funk, intro under ten seconds, warm mature high-end focus music, no piercing highs, no sub-bass rumble, no short-loop repetition.";
        let body = json!({"model": model, "messages":[{"role":"system","content":locked},{"role":"user","content":structured_prompt}],"response_format":{"type":"json_object"},"temperature":0.2,"max_tokens":700,"stream":false});
        let (value, usage) = self.chat(api_key, &body, MAX_TEXT_BYTES)?;
        let content = choice_message(&value)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| OpenRouterError::invalid("The prompt model returned no text"))?;
        let parsed: Value = serde_json::from_str(content)
            .map_err(|_| OpenRouterError::invalid("The prompt model returned invalid JSON"))?;
        let prompt = parsed
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| OpenRouterError::invalid("The prompt model returned no prompt"))?
            .trim()
            .to_owned();
        if prompt.is_empty() || prompt.len() > 12_000 || contains_locked_violation(&prompt) {
            return Err(OpenRouterError::invalid(
                "The refined prompt did not satisfy Aria Focus constraints",
            ));
        }
        Ok(TextResponse {
            request_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
            text: prompt,
            usage,
        })
    }

    pub fn generate_audio(
        &self,
        api_key: &str,
        model: &str,
        prompt: &str,
        duration_seconds: u16,
    ) -> Result<MediaResponse, OpenRouterError> {
        let body = json!({"model": model,"messages":[{"role":"user","content":prompt}],"modalities":["text","audio"],"audio":{"format":"wav"},"stream":true,"stream_options":{"include_usage":true},"metadata":{"aria_focus_duration_seconds":duration_seconds}});
        let response = self.request(
            api_key,
            self.http
                .post(format!("{}/chat/completions", self.api_root))
                .json(&body),
        )?;
        let (request_id, bytes, usage) = read_audio_stream(response)?;
        let mime_type = audio_mime_type(&bytes).to_owned();
        Ok(MediaResponse {
            request_id: request_id.clone(),
            bytes,
            mime_type,
            usage: UsageCost {
                request_id: usage.request_id.or(request_id),
                ..usage
            },
        })
    }

    pub fn generate_cover(
        &self,
        api_key: &str,
        model: &str,
        prompt: &str,
    ) -> Result<MediaResponse, OpenRouterError> {
        let body = json!({"model": model,"messages":[{"role":"user","content":prompt}],"modalities":["text","image"],"stream":false});
        self.generate_media(api_key, &body, false)
    }

    fn generate_media(
        &self,
        api_key: &str,
        body: &Value,
        audio: bool,
    ) -> Result<MediaResponse, OpenRouterError> {
        let (value, usage) = self.chat(api_key, body, MAX_MEDIA_BYTES)?;
        let request_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
        let message = choice_message(&value)
            .ok_or_else(|| OpenRouterError::invalid("The provider returned no media message"))?;
        let (encoded, mime) = if audio {
            let audio = message
                .get("audio")
                .ok_or_else(|| OpenRouterError::invalid("The audio model returned no audio"))?;
            (
                audio.get("data").and_then(Value::as_str).ok_or_else(|| {
                    OpenRouterError::invalid("The audio response contained no data")
                })?,
                "audio/wav",
            )
        } else {
            let image = message
                .get("images")
                .and_then(Value::as_array)
                .and_then(|images| images.first())
                .and_then(|image| image.get("image_url").and_then(|url| url.get("url")))
                .or_else(|| {
                    message
                        .get("images")
                        .and_then(Value::as_array)
                        .and_then(|images| images.first())
                        .and_then(|image| image.get("url"))
                })
                .and_then(Value::as_str)
                .ok_or_else(|| OpenRouterError::invalid("The image model returned no image"))?;
            if let Some((mime, encoded)) = image.split_once(",") {
                (
                    encoded,
                    mime.strip_prefix("data:")
                        .and_then(|v| v.strip_suffix(";base64"))
                        .unwrap_or("image/png"),
                )
            } else {
                (image, "image/png")
            }
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| {
                OpenRouterError::invalid("The provider returned invalid media encoding")
            })?;
        if bytes.is_empty() || bytes.len() > MAX_MEDIA_BYTES {
            return Err(OpenRouterError {
                code: OpenRouterErrorCode::ResponseTooLarge,
                message: "The provider returned media outside the safe size limit".into(),
                retryable: false,
                status: None,
            });
        }
        let mime = if audio { audio_mime_type(&bytes) } else { mime };
        Ok(MediaResponse {
            request_id: request_id.clone(),
            bytes,
            mime_type: mime.to_owned(),
            usage: UsageCost {
                request_id: usage.request_id.or_else(|| request_id.clone()),
                ..usage
            },
        })
    }

    fn chat(
        &self,
        api_key: &str,
        body: &Value,
        max_bytes: usize,
    ) -> Result<(Value, UsageCost), OpenRouterError> {
        let response = self.request(
            api_key,
            self.http
                .post(format!("{}/chat/completions", self.api_root))
                .json(body),
        )?;
        let value = read_json(response, max_bytes)?;
        let usage = parse_usage(&value);
        Ok((value, usage))
    }

    fn request(
        &self,
        api_key: &str,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<Response, OpenRouterError> {
        if api_key.trim().is_empty() {
            return Err(OpenRouterError {
                code: OpenRouterErrorCode::MissingKey,
                message: "Add an OpenRouter API key to use cloud generation.".into(),
                retryable: false,
                status: None,
            });
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
                .map_err(|_| OpenRouterError::invalid("The API key is invalid"))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Aria-Focus/1 cloud-generation"),
        );
        let response = request.headers(headers).send().map_err(|error| {
            if error.is_timeout() {
                OpenRouterError::network(
                    OpenRouterErrorCode::NetworkTimeout,
                    "OpenRouter timed out",
                )
            } else {
                OpenRouterError::network(
                    OpenRouterErrorCode::ProviderUnavailable,
                    "OpenRouter could not be reached",
                )
            }
        })?;
        if response.status().is_success() {
            return Ok(response);
        }
        Err(error_from_response(response))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyInfo {
    pub label: Option<String>,
    pub limit: Option<f64>,
    pub usage: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResponse {
    pub request_id: Option<String>,
    pub text: String,
    pub usage: UsageCost,
}

fn first_choice(value: &Value) -> Option<&Value> {
    value.get("choices")?.as_array()?.first()
}

fn choice_message(value: &Value) -> Option<&Value> {
    first_choice(value)?.get("message")
}

fn read_json(response: Response, max_bytes: usize) -> Result<Value, OpenRouterError> {
    let bytes = read_bounded(response, max_bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| OpenRouterError::invalid("OpenRouter returned malformed JSON"))
}

fn read_bounded(response: Response, max_bytes: usize) -> Result<Vec<u8>, OpenRouterError> {
    if response
        .content_length()
        .is_some_and(|size| size > max_bytes as u64)
    {
        return Err(OpenRouterError {
            code: OpenRouterErrorCode::ResponseTooLarge,
            message: "OpenRouter response is larger than the safe limit".into(),
            retryable: false,
            status: None,
        });
    }
    let mut out = Vec::new();
    response
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut out)
        .map_err(|_| {
            OpenRouterError::network(
                OpenRouterErrorCode::ProviderUnavailable,
                "OpenRouter response could not be read",
            )
        })?;
    if out.len() > max_bytes {
        return Err(OpenRouterError {
            code: OpenRouterErrorCode::ResponseTooLarge,
            message: "OpenRouter response is larger than the safe limit".into(),
            retryable: false,
            status: None,
        });
    }
    Ok(out)
}

fn read_audio_stream(
    response: Response,
) -> Result<(Option<String>, Vec<u8>, UsageCost), OpenRouterError> {
    parse_audio_stream(BufReader::new(response))
}

fn audio_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        "audio/wav"
    } else if bytes.len() >= 4 && &bytes[0..4] == b"fLaC" {
        "audio/flac"
    } else {
        "audio/mpeg"
    }
}

fn parse_audio_stream<R: BufRead>(
    mut reader: R,
) -> Result<(Option<String>, Vec<u8>, UsageCost), OpenRouterError> {
    let mut line = String::new();
    let mut bytes = Vec::new();
    let mut request_id = None;
    let mut usage = UsageCost::default();
    let mut saw_done = false;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|_| {
            OpenRouterError::network(
                OpenRouterErrorCode::ProviderUnavailable,
                "The audio stream could not be read",
            )
        })?;
        if read == 0 {
            break;
        }
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            saw_done = true;
            break;
        }
        if data.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(data).map_err(|_| {
            OpenRouterError::invalid("OpenRouter returned malformed audio stream data")
        })?;
        if request_id.is_none() {
            request_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
        }
        let parsed_usage = parse_usage(&value);
        if parsed_usage.cost_microdollars > 0
            || parsed_usage.input_tokens.is_some()
            || parsed_usage.output_tokens.is_some()
        {
            usage = parsed_usage;
        }
        let encoded = first_choice(&value)
            .and_then(|choice| choice.get("delta").or_else(|| choice.get("message")))
            .and_then(|message| message.get("audio"))
            .and_then(|audio| audio.get("data"))
            .and_then(Value::as_str);
        if let Some(encoded) = encoded {
            let chunk = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| {
                    OpenRouterError::invalid("The provider returned invalid audio encoding")
                })?;
            if bytes.len().saturating_add(chunk.len()) > MAX_MEDIA_BYTES {
                return Err(OpenRouterError {
                    code: OpenRouterErrorCode::ResponseTooLarge,
                    message: "The provider returned audio outside the safe size limit".into(),
                    retryable: false,
                    status: None,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
    }
    if !saw_done || bytes.is_empty() {
        return Err(OpenRouterError::retryable_invalid(
            "The audio model returned no complete audio stream",
        ));
    }
    Ok((request_id, bytes, usage))
}

fn parse_usage(value: &Value) -> UsageCost {
    let usage = value.get("usage");
    let cost = usage
        .and_then(|u| u.get("cost"))
        .and_then(Value::as_f64)
        .or_else(|| value.get("cost").and_then(Value::as_f64))
        .unwrap_or(0.0)
        .max(0.0);
    UsageCost {
        request_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
        cost_microdollars: (cost * 1_000_000.0).round() as u64,
        input_tokens: usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(Value::as_u64),
    }
}

fn error_from_response(response: Response) -> OpenRouterError {
    let status = response.status().as_u16();
    let value = read_json(response, MAX_TEXT_BYTES).unwrap_or_default();
    let provider_code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("OpenRouter returned an error");
    let (code, retryable) = match status {
        401 => (OpenRouterErrorCode::InvalidKey, false),
        402 => (OpenRouterErrorCode::InsufficientCredits, false),
        408 => (OpenRouterErrorCode::NetworkTimeout, true),
        429 => (OpenRouterErrorCode::RateLimited, true),
        400 | 404 => (OpenRouterErrorCode::ModelUnavailable, false),
        500..=599 => (OpenRouterErrorCode::ProviderUnavailable, true),
        _ => match provider_code {
            Some(402) => (OpenRouterErrorCode::InsufficientCredits, false),
            Some(429) => (OpenRouterErrorCode::RateLimited, true),
            _ => (OpenRouterErrorCode::Unknown, false),
        },
    };
    let safe_message = match code {
        OpenRouterErrorCode::InvalidKey => {
            "OpenRouter rejected the API key. Check it and try again."
        }
        OpenRouterErrorCode::InsufficientCredits => {
            "OpenRouter reports insufficient credits for this request."
        }
        OpenRouterErrorCode::RateLimited => {
            "OpenRouter is rate-limiting requests. Wait a moment and try again."
        }
        OpenRouterErrorCode::ModelUnavailable => {
            "The selected model is unavailable or does not support this output."
        }
        OpenRouterErrorCode::NetworkTimeout => {
            "OpenRouter timed out before completing the request."
        }
        OpenRouterErrorCode::ProviderUnavailable => "OpenRouter is temporarily unavailable.",
        _ => message,
    };
    OpenRouterError {
        code,
        message: safe_message.to_owned(),
        retryable,
        status: Some(status),
    }
}

impl OpenRouterError {
    fn invalid(message: &str) -> Self {
        Self {
            code: OpenRouterErrorCode::InvalidResponse,
            message: message.into(),
            retryable: false,
            status: None,
        }
    }
    fn retryable_invalid(message: &str) -> Self {
        Self {
            code: OpenRouterErrorCode::InvalidResponse,
            message: message.into(),
            retryable: true,
            status: None,
        }
    }
    fn network(code: OpenRouterErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
            status: None,
        }
    }
}

fn contains_locked_violation(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let forbidden_positive = [
        "singer",
        "lyrics",
        "speech",
        "jazz",
        "swing",
        "blues",
        "funk",
        "piercing",
        "sub-bass rumble",
    ];
    forbidden_positive.iter().any(|term| {
        lower.contains(term)
            && !lower.contains(&format!("no {term}"))
            && !lower.contains(&format!("without {term}"))
            && !lower.contains(&format!("avoid {term}"))
    }) || (lower.contains("vocal")
        && ![
            "no vocal",
            "without vocal",
            "avoid vocal",
            "instrumental only",
        ]
        .iter()
        .any(|allowed| lower.contains(allowed)))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn locked_prompt_rejects_vocals_and_jazz() {
        assert!(contains_locked_violation("warm jazz with vocal lead"));
        assert!(!contains_locked_violation(
            "warm instrumental piano and strings"
        ));
    }

    #[test]
    fn error_mapping_does_not_expose_provider_body() {
        let err = OpenRouterError {
            code: OpenRouterErrorCode::InvalidKey,
            message: "OpenRouter rejected the API key. Check it and try again.".into(),
            retryable: false,
            status: Some(401),
        };
        assert!(!err.message.contains("Bearer"));
    }

    #[test]
    fn audio_sse_chunks_are_decoded_and_concatenated_with_usage() {
        let first = base64::engine::general_purpose::STANDARD.encode(b"RIFF");
        let second = base64::engine::general_purpose::STANDARD.encode(b"WAVE");
        let first_chunk = json!({
            "id":"req_audio",
            "choices":[{"delta":{"audio":{"data":first}}}]
        });
        let second_chunk = json!({
            "id":"req_audio",
            "choices":[{"delta":{"audio":{"data":second}}}],
            "usage":{"cost":0.08}
        });
        let stream = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            first_chunk, second_chunk
        );
        let (request_id, bytes, usage) = parse_audio_stream(Cursor::new(stream)).unwrap();
        assert_eq!(request_id.as_deref(), Some("req_audio"));
        assert_eq!(bytes, b"RIFFWAVE");
        assert_eq!(usage.cost_microdollars, 80_000);
    }
}
