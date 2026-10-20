//! Public ordered input and audited request-wide native processing controls.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
pub use veoveo_mcp_artifact::contract::ArtifactId;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub enum Dimensions {
    D128,
    D256,
    D512,
    #[default]
    D768,
}
impl From<Dimensions> for usize {
    fn from(value: Dimensions) -> Self {
        match value {
            Dimensions::D128 => 128,
            Dimensions::D256 => 256,
            Dimensions::D512 => 512,
            Dimensions::D768 => 768,
        }
    }
}
impl TryFrom<usize> for Dimensions {
    type Error = &'static str;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            128 => Ok(Self::D128),
            256 => Ok(Self::D256),
            512 => Ok(Self::D512),
            768 => Ok(Self::D768),
            _ => Err("dimensions must be 128, 256, 512 or 768"),
        }
    }
}
impl JsonSchema for Dimensions {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Dimensions".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"integer", "enum":[128,256,512,768]})
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PromptName {
    SearchQuery,
    CodeRetrieval,
    Document,
    QuestionAnswering,
    FactChecking,
    Classification,
    Clustering,
    SentenceSimilarity,
}
impl PromptName {
    pub const ALL: [Self; 8] = [
        Self::SearchQuery,
        Self::CodeRetrieval,
        Self::Document,
        Self::QuestionAnswering,
        Self::FactChecking,
        Self::Classification,
        Self::Clustering,
        Self::SentenceSimilarity,
    ];
    pub fn prefix(self) -> &'static str {
        match self {
            Self::SearchQuery => "task: search result | query: ",
            Self::CodeRetrieval => "task: code retrieval | query: ",
            Self::Document => "title: none | text: ",
            Self::QuestionAnswering => "task: question answering | query: ",
            Self::FactChecking => "task: fact checking | query: ",
            Self::Classification => "task: classification | query: ",
            Self::Clustering => "task: clustering | query: ",
            Self::SentenceSimilarity => "task: sentence similarity | query: ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        #[serde(rename = "artifactId")]
        artifact_id: ArtifactId,
    },
    Audio {
        #[serde(rename = "artifactId")]
        artifact_id: ArtifactId,
    },
    Video {
        #[serde(rename = "artifactId")]
        artifact_id: ArtifactId,
    },
}
impl ContentPart {
    pub fn modality(&self) -> Modality {
        match self {
            Self::Text { .. } => Modality::Text,
            Self::Image { .. } => Modality::Image,
            Self::Audio { .. } => Modality::Audio,
            Self::Video { .. } => Modality::Video,
        }
    }
    pub fn artifact_id(&self) -> Option<ArtifactId> {
        match self {
            Self::Text { .. } => None,
            Self::Image { artifact_id }
            | Self::Audio { artifact_id }
            | Self::Video { artifact_id } => Some(*artifact_id),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingInput {
    /// Ordered parts jointly embedded into one vector. Repeated modalities are allowed.
    #[schemars(length(min = 1, max = 16))]
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOptions {
    #[serde(default = "image_tokens")]
    #[schemars(schema_with = "soft_tokens_schema")]
    pub max_soft_tokens: usize,
}
fn image_tokens() -> usize {
    280
}
impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            max_soft_tokens: 280,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioOptions {
    /// Native truncation is off by default; enabling it makes retained coverage explicit.
    #[serde(default)]
    pub truncation: bool,
    /// Maximum retained mono 16 kHz samples, only with truncation=true; default 480000 when enabled.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_value"
    )]
    #[schemars(schema_with = "audio_length_schema")]
    pub max_length: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OverflowStrategy {
    #[default]
    Uniform,
    Truncate,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoOptions {
    /// Requested native sampling frames/second; selection is also bounded by maxFrames.
    #[serde(default = "video_fps")]
    #[schemars(schema_with = "fps_schema")]
    pub fps: f64,
    /// Native selected-frame budget, not a source-media clipping operation.
    #[serde(default = "video_frames")]
    #[schemars(range(min = 1, max = 256))]
    pub max_frames: usize,
    /// Native uniform resampling or truncation when the requested samples overflow the budget.
    #[serde(default)]
    pub overflow_strategy: OverflowStrategy,
    #[serde(default = "video_tokens")]
    #[schemars(schema_with = "soft_tokens_schema")]
    pub max_soft_tokens: usize,
    /// Include the native index/source-FPS timestamps in the model prompt.
    #[serde(default)]
    pub add_timestamps: bool,
    /// Include the native video soundtrack. Cannot be combined with separate audio parts anywhere in the batch.
    #[serde(default)]
    pub load_audio_from_video: bool,
}
fn video_fps() -> f64 {
    1.0
}
fn video_frames() -> usize {
    32
}
fn video_tokens() -> usize {
    140
}
impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            fps: 1.0,
            max_frames: 32,
            overflow_strategy: OverflowStrategy::Uniform,
            max_soft_tokens: 140,
            add_timestamps: false,
            load_audio_from_video: false,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preprocessing {
    #[serde(default)]
    pub image: ImageOptions,
    #[serde(default)]
    pub audio: AudioOptions,
    #[serde(default)]
    pub video: VideoOptions,
}
impl Preprocessing {
    pub fn effective(&self) -> Self {
        let mut value = self.clone();
        if value.audio.truncation && value.audio.max_length.is_none() {
            value.audio.max_length = Some(480000);
        }
        value
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !SOFT_TOKENS.contains(&self.image.max_soft_tokens)
            || !SOFT_TOKENS.contains(&self.video.max_soft_tokens)
            || !self.video.fps.is_finite()
            || self.video.fps <= 0.0
            || self.video.fps > MAX_FPS
            || self.video.max_frames == 0
            || self.video.max_frames > MAX_FRAMES
            || (self.audio.max_length.is_some() && !self.audio.truncation)
            || self
                .audio
                .max_length
                .is_some_and(|n| n == 0 || n > MAX_SOURCE_SECONDS * 16000)
        {
            return Err("invalid native preprocessing option");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[veoveo_hub_sdk::tool(provider = EmbeddingProvider, output = EmbeddingResult, name = "embed")]
pub struct EmbedRequest {
    /// Ordered independent items: N items produce N ordered vectors; each item's parts produce one joint vector.
    #[schemars(length(min = 1, max = 8))]
    pub items: Vec<EmbeddingInput>,
    /// Matryoshka dimensions shared by every vector; default 768, then normalized.
    #[serde(default)]
    pub dimensions: Dimensions,
    /// Optional native request-wide task prefix; exclusive with prompt. Omission means an empty prefix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_value"
    )]
    #[schemars(with = "PromptName")]
    pub prompt_name: Option<PromptName>,
    /// Optional literal prefix applied once to each complete item; exclusive with promptName. Not echoed in output.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_value"
    )]
    #[schemars(with = "String")]
    pub prompt: Option<String>,
    /// Closed request-wide native preprocessing controls. Service bounds are separate from Google capabilities.
    #[serde(default)]
    pub preprocessing: Preprocessing,
}
impl EmbedRequest {
    pub fn effective_prompt(&self) -> &str {
        self.prompt
            .as_deref()
            .unwrap_or_else(|| self.prompt_name.map_or("", PromptName::prefix))
    }
    pub fn validate(&self) -> Result<usize, &'static str> {
        self.preprocessing.validate()?;
        if self.prompt.is_some() && self.prompt_name.is_some() {
            return Err("prompt and promptName are mutually exclusive");
        }
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err("batch must contain 1 to 8 items");
        }
        let mut bytes = self.effective_prompt().len();
        let mut parts = 0;
        let mut has_audio = false;
        if reserved(self.effective_prompt()) {
            return Err("media placeholders are reserved for ordered content parts");
        }
        for item in &self.items {
            if item.content.is_empty() || item.content.len() > MAX_PARTS_PER_ITEM {
                return Err("item must contain 1 to 16 ordered parts");
            }
            parts += item.content.len();
            for part in &item.content {
                if let ContentPart::Text { text } = part {
                    if text.trim().is_empty() || reserved(text) {
                        return Err(
                            "text must be nonblank and contain no reserved media placeholders",
                        );
                    }
                    bytes = bytes
                        .checked_add(text.len())
                        .ok_or("content size overflow")?;
                }
                has_audio |= matches!(part, ContentPart::Audio { .. });
            }
        }
        if parts > MAX_PARTS || bytes > MAX_CONTENT_BYTES {
            return Err("batch content exceeds service bounds");
        }
        if has_audio && self.preprocessing.video.load_audio_from_video {
            return Err("video soundtrack inclusion cannot be combined with separate audio parts");
        }
        Ok(bytes)
    }
}
fn reserved(value: &str) -> bool {
    ["<|image|>", "<|audio|>", "<|video|>"]
        .iter()
        .any(|marker| value.contains(marker))
}

fn optional_value<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}
fn soft_tokens_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"integer","enum":[70,140,280,560,1120]})
}
fn audio_length_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"integer","minimum":1,"maximum":4800000})
}
fn fps_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"number","exclusiveMinimum":0,"maximum":120})
}
