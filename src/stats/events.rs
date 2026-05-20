use super::reports::TokenUsage;
use chrono::{DateTime, TimeZone, Utc};
use serde::de::IgnoredAny;
use serde::Deserialize;
use std::borrow::Cow;

pub(super) fn parse_usage_json_event(
    line: &str,
) -> Result<Option<UsageJsonEvent<'_>>, serde_json::Error> {
    match serde_json::from_str::<UsageJsonEvent>(line) {
        Ok(event) => Ok(Some(event)),
        Err(error) => {
            if serde_json::from_str::<IgnoredAny>(line).is_ok() {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonObject<T> {
    Object(T),
    Other(IgnoredAny),
}

impl<T> JsonObject<T> {
    fn as_object(&self) -> Option<&T> {
        match self {
            Self::Object(value) => Some(value),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonString<'a> {
    String(#[serde(borrow)] Cow<'a, str>),
    Other(IgnoredAny),
}

impl JsonString<'_> {
    fn as_non_empty_str(&self) -> Option<&str> {
        match self {
            Self::String(value) if !value.trim().is_empty() => Some(value.as_ref()),
            Self::String(_) | Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonI64 {
    I64(i64),
    U64(u64),
    F64(f64),
    Other(IgnoredAny),
}

impl JsonI64 {
    fn to_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            Self::U64(value) => Some(*value as i64),
            Self::F64(value) => Some(*value as i64),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonDate<'a> {
    String(#[serde(borrow)] Cow<'a, str>),
    I64(i64),
    U64(u64),
    F64(f64),
    Other(IgnoredAny),
}

impl JsonDate<'_> {
    fn to_utc(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::String(value) => DateTime::parse_from_rfc3339(value.as_ref())
                .ok()
                .map(|date| date.with_timezone(&Utc)),
            Self::I64(value) => Utc.timestamp_millis_opt(*value).single(),
            Self::U64(value) => Utc.timestamp_millis_opt(*value as i64).single(),
            Self::F64(value) => Utc.timestamp_millis_opt(*value as i64).single(),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct UsageJsonEvent<'a> {
    #[serde(rename = "type", default, borrow)]
    event_type: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    timestamp: Option<JsonDate<'a>>,
    #[serde(default, borrow)]
    payload: Option<JsonObject<UsageJsonPayload<'a>>>,
}

impl<'a> UsageJsonEvent<'a> {
    pub(super) fn event_type(&self) -> Option<&str> {
        self.event_type
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
    }

    pub(super) fn timestamp(&self) -> Option<DateTime<Utc>> {
        self.timestamp.as_ref().and_then(JsonDate::to_utc)
    }

    pub(super) fn payload(&self) -> Option<&UsageJsonPayload<'a>> {
        self.payload.as_ref().and_then(JsonObject::as_object)
    }
}

#[derive(Deserialize)]
pub(super) struct UsageJsonPayload<'a> {
    #[serde(rename = "type", default, borrow)]
    payload_type: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    id: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    model: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    cwd: Option<JsonString<'a>>,
    #[serde(default, alias = "reasoningEffort", borrow)]
    reasoning_effort: Option<JsonString<'a>>,
    #[serde(default, alias = "modelReasoningEffort", borrow)]
    model_reasoning_effort: Option<JsonString<'a>>,
    #[serde(default, alias = "modelConfig", borrow)]
    model_config: Option<JsonObject<ReasoningJsonFields<'a>>>,
    #[serde(default, borrow)]
    reasoning: Option<JsonObject<ReasoningJsonFields<'a>>>,
    #[serde(default, alias = "collaborationMode", borrow)]
    collaboration_mode: Option<JsonObject<CollaborationModeJson<'a>>>,
    #[serde(default)]
    info: Option<JsonObject<TokenCountInfoJson>>,
}

impl<'a> UsageJsonPayload<'a> {
    pub(super) fn payload_type(&self) -> Option<&str> {
        self.payload_type
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
    }

    pub(super) fn id(&self) -> Option<&str> {
        self.id.as_ref().and_then(JsonString::as_non_empty_str)
    }

    pub(super) fn model(&self) -> Option<&str> {
        self.model.as_ref().and_then(JsonString::as_non_empty_str)
    }

    pub(super) fn cwd(&self) -> Option<&str> {
        self.cwd.as_ref().and_then(JsonString::as_non_empty_str)
    }

    pub(super) fn info(&self) -> Option<&TokenCountInfoJson> {
        self.info.as_ref().and_then(JsonObject::as_object)
    }

    pub(super) fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
            .or_else(|| {
                self.model_reasoning_effort
                    .as_ref()
                    .and_then(JsonString::as_non_empty_str)
            })
            .or_else(|| {
                self.model_config
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(ReasoningJsonFields::reasoning_effort)
            })
            .or_else(|| {
                self.reasoning
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(ReasoningJsonFields::reasoning_effort)
            })
            .or_else(|| {
                self.collaboration_mode
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(CollaborationModeJson::reasoning_effort)
            })
    }
}

#[derive(Deserialize)]
struct CollaborationModeJson<'a> {
    #[serde(default, borrow)]
    settings: Option<JsonObject<ReasoningJsonFields<'a>>>,
}

impl CollaborationModeJson<'_> {
    fn reasoning_effort(&self) -> Option<&str> {
        self.settings
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(ReasoningJsonFields::reasoning_effort)
    }
}

#[derive(Deserialize)]
struct ReasoningJsonFields<'a> {
    #[serde(default, borrow)]
    effort: Option<JsonString<'a>>,
    #[serde(default, alias = "reasoningEffort", borrow)]
    reasoning_effort: Option<JsonString<'a>>,
}

impl ReasoningJsonFields<'_> {
    fn reasoning_effort(&self) -> Option<&str> {
        self.effort
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
            .or_else(|| {
                self.reasoning_effort
                    .as_ref()
                    .and_then(JsonString::as_non_empty_str)
            })
    }
}

#[derive(Deserialize)]
pub(super) struct TokenCountInfoJson {
    #[serde(default, alias = "totalTokenUsage")]
    total_token_usage: Option<JsonObject<TokenUsageJson>>,
    #[serde(default, alias = "lastTokenUsage")]
    last_token_usage: Option<JsonObject<TokenUsageJson>>,
}

impl TokenCountInfoJson {
    pub(super) fn total_token_usage(&self) -> Option<TokenUsage> {
        self.total_token_usage
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(TokenUsageJson::to_token_usage)
    }

    pub(super) fn last_token_usage(&self) -> Option<TokenUsage> {
        self.last_token_usage
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(TokenUsageJson::to_token_usage)
    }
}

#[derive(Deserialize)]
struct TokenUsageJson {
    #[serde(default, alias = "inputTokens")]
    input_tokens: Option<JsonI64>,
    #[serde(default, alias = "cachedInputTokens")]
    cached_input_tokens: Option<JsonI64>,
    #[serde(default, alias = "outputTokens")]
    output_tokens: Option<JsonI64>,
    #[serde(default, alias = "reasoningOutputTokens")]
    reasoning_output_tokens: Option<JsonI64>,
    #[serde(default, alias = "totalTokens")]
    total_tokens: Option<JsonI64>,
}

impl TokenUsageJson {
    fn to_token_usage(&self) -> Option<TokenUsage> {
        let input = self.input_tokens.as_ref().and_then(JsonI64::to_i64);
        let output = self.output_tokens.as_ref().and_then(JsonI64::to_i64);
        let total = self.total_tokens.as_ref().and_then(JsonI64::to_i64);

        if input.is_none() && output.is_none() && total.is_none() {
            return None;
        }

        Some(TokenUsage {
            input_tokens: input.unwrap_or(0),
            cached_input_tokens: self
                .cached_input_tokens
                .as_ref()
                .and_then(JsonI64::to_i64)
                .unwrap_or(0),
            output_tokens: output.unwrap_or(0),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .as_ref()
                .and_then(JsonI64::to_i64)
                .unwrap_or(0),
            total_tokens: total.unwrap_or_else(|| input.unwrap_or(0) + output.unwrap_or(0)),
        })
    }
}
