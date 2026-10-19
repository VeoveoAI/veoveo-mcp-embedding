use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MODEL_ID: &str = "google/embeddinggemma-2";
pub const MODEL_REVISION: &str = "914f7f89142e33e77833254d9c9b90c3cef7303b";
pub const PROCESSING_ID: &str =
    "sha256:e570e2bd489dbf11cf9da6de0e21b5ba06123d6a205c69aa1312495c3ac6e4a4";
pub const DIMENSIONS: usize = 768;
pub const MAX_ITEMS: usize = 8;
pub const MAX_CONTENT_BYTES: usize = 65536;
pub const MAX_ITEM_TOKENS: u64 = 8192;
pub const MAX_BATCH_TOKENS: u64 = 16384;
pub const MAX_JSON_BYTES: usize = 524288;
pub const MAX_RESPONSE_BYTES: usize = 1048576;
pub const REQUEST_SECONDS: u64 = 60;

/// Exactly one upstream prompt purpose; unknown members are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "purpose", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingInput {
    SearchQuery {
        text: String,
    },
    CodeQuery {
        text: String,
    },
    Document {
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
}
impl EmbeddingInput {
    pub fn purpose(&self) -> Purpose {
        match self {
            Self::SearchQuery { .. } => Purpose::SearchQuery,
            Self::CodeQuery { .. } => Purpose::CodeQuery,
            Self::Document { .. } => Purpose::Document,
        }
    }
    fn content(&self) -> (&str, Option<&str>) {
        match self {
            Self::SearchQuery { text } | Self::CodeQuery { text } => (text, None),
            Self::Document { content, title } => (content, title.as_deref()),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    SearchQuery,
    CodeQuery,
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[veoveo_hub_sdk::tool(provider = super::EmbeddingProvider, output = EmbeddingResult, name = "embed")]
pub struct EmbedRequest {
    #[schemars(length(min = 1, max = 8))]
    pub items: Vec<EmbeddingInput>,
}
impl EmbedRequest {
    /// Byte admission is conservative; actual tokenizer limits belong to the runtime.
    pub fn validate(&self) -> Result<usize, &'static str> {
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err("batch must contain 1 to 8 items");
        }
        let mut bytes = 0_usize;
        for item in &self.items {
            let (text, title) = item.content();
            if text.trim().is_empty() || title.is_some_and(|v| v.trim().is_empty()) {
                return Err("content and supplied titles must not be blank");
            }
            bytes = bytes
                .checked_add(text.len())
                .and_then(|n| n.checked_add(title.map_or(0, str::len)))
                .ok_or("content size overflow")?;
        }
        if bytes > MAX_CONTENT_BYTES {
            return Err("batch content exceeds 65536 UTF-8 bytes");
        }
        Ok(bytes)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelIdentity {
    pub id: String,
    pub revision: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    Mps,
    Cuda,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution {
    pub device: Device,
    pub dtype: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeIdentity {
    pub package: String,
    pub version: String,
    pub source_revision: String,
    pub python: String,
    pub sentence_transformers: String,
    pub transformers: String,
    pub torch: String,
    pub torchvision: String,
    pub pillow: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub max_items: usize,
    pub max_content_bytes: usize,
    pub max_item_tokens: u64,
    pub max_batch_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, veoveo_hub_sdk::SingleOutput)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub model: ModelIdentity,
    pub processing_id: String,
    pub dimensions: usize,
    pub normalized: bool,
    pub execution: Execution,
    pub runtime: RuntimeIdentity,
    pub purposes: Vec<Purpose>,
    pub limits: Limits,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Embedding {
    pub index: usize,
    pub purpose: Purpose,
    pub vector: Vec<f32>,
    pub token_count: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, veoveo_hub_sdk::SingleOutput)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingResult {
    pub request_id: uuid::Uuid,
    pub model: ModelIdentity,
    pub processing_id: String,
    pub dimensions: usize,
    pub normalized: bool,
    pub execution: Execution,
    pub runtime: RuntimeIdentity,
    pub items: Vec<Embedding>,
    pub usage: TokenUsage,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRequest {
    pub request_id: uuid::Uuid,
    pub items: Vec<EmbeddingInput>,
}

/// Closed private-runtime rejection vocabulary. Arbitrary upstream error text is never exposed.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    #[error("invalid_request")]
    InvalidRequest,
    #[error("invalid_batch")]
    InvalidBatch,
    #[error("invalid_purpose")]
    InvalidPurpose,
    #[error("invalid_item")]
    InvalidItem,
    #[error("empty_content")]
    EmptyContent,
    #[error("invalid_title")]
    InvalidTitle,
    #[error("invalid_unicode")]
    InvalidUnicode,
    #[error("content_limit")]
    ContentLimit,
    #[error("item_token_limit")]
    ItemTokenLimit,
    #[error("batch_token_limit")]
    BatchTokenLimit,
    #[error("request_size")]
    RequestSize,
    #[error("invalid_json")]
    InvalidJson,
    #[error("busy")]
    Busy,
    #[error("unauthorized")]
    Unauthorized,
    #[error("not_ready")]
    NotReady,
}
impl RejectionCode {
    pub fn status(self) -> u16 {
        match self {
            Self::InvalidJson => 400,
            Self::Unauthorized => 401,
            Self::ContentLimit | Self::RequestSize => 413,
            Self::Busy => 429,
            Self::NotReady => 503,
            _ => 422,
        }
    }
    pub fn is_input_error(self) -> bool {
        !matches!(self, Self::Unauthorized | Self::Busy | Self::NotReady)
    }
}
