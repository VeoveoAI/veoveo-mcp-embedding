//! Closed public results, provenance and defensive Veoveo service limits.
use super::{ArtifactId, Dimensions, Modality, Preprocessing, PromptName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_mcp_artifact::contract::{MediaType, ObjectSize};
use veoveo_types::Sha256Digest;

pub const MODEL_ID: &str = "google/embeddinggemma-2";
pub const MODEL_REVISION: &str = "914f7f89142e33e77833254d9c9b90c3cef7303b";
pub const DIMENSIONS: usize = 768;
pub const MAX_ITEMS: usize = 8;
pub const MAX_PARTS_PER_ITEM: usize = 16;
pub const MAX_PARTS: usize = 32;
pub const MAX_CONTENT_BYTES: usize = 65536;
pub const MAX_ITEM_TOKENS: u64 = 8192;
pub const MAX_BATCH_TOKENS: u64 = 16384;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TRANSFER_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 16_000_000;
pub const MAX_TOTAL_PIXELS: u64 = 64_000_000;
pub const MAX_SOURCE_SECONDS: u64 = 300;
pub const MAX_AUDIO_SECONDS: u64 = 600;
pub const MAX_FPS: f64 = 120.0;
pub const MAX_FRAMES: usize = 256;
pub const SOFT_TOKENS: [usize; 5] = [70, 140, 280, 560, 1120];
pub const MAX_JSON_BYTES: usize = 524288;
pub const MAX_RESPONSE_BYTES: usize = 1048576;
pub const REQUEST_SECONDS: u64 = 60;

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
pub struct CapabilityExecution {
    pub device: Device,
    pub dtype: String,
    pub batch_size: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FfmpegIdentity {
    pub version: String,
    pub libavfilter: [u32; 3],
    pub libavutil: [u32; 3],
    pub libavcodec: [u32; 3],
    pub libavformat: [u32; 3],
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
    pub torchcodec: String,
    pub ffmpeg: FfmpegIdentity,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub max_items: usize,
    pub max_parts_per_item: usize,
    pub max_parts: usize,
    pub max_content_bytes: usize,
    pub max_item_tokens: u64,
    pub max_batch_tokens: u64,
    pub max_artifact_bytes: u64,
    pub max_transfer_bytes: u64,
    pub max_pixels: u64,
    pub max_total_pixels: u64,
    pub max_source_seconds: u64,
    pub max_audio_seconds: u64,
    pub max_video_fps: f64,
    pub max_video_frames: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_items: MAX_ITEMS,
            max_parts_per_item: MAX_PARTS_PER_ITEM,
            max_parts: MAX_PARTS,
            max_content_bytes: MAX_CONTENT_BYTES,
            max_item_tokens: MAX_ITEM_TOKENS,
            max_batch_tokens: MAX_BATCH_TOKENS,
            max_artifact_bytes: MAX_ARTIFACT_BYTES,
            max_transfer_bytes: MAX_TRANSFER_BYTES,
            max_pixels: MAX_PIXELS,
            max_total_pixels: MAX_TOTAL_PIXELS,
            max_source_seconds: MAX_SOURCE_SECONDS,
            max_audio_seconds: MAX_AUDIO_SECONDS,
            max_video_fps: MAX_FPS,
            max_video_frames: MAX_FRAMES,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessingSemantics {
    pub limits: String,
    pub preprocessing: String,
    pub dimensions: String,
    pub video_timestamps: String,
    pub audio_from_video: String,
    pub image_frames: String,
    pub text_placeholders: String,
}
impl Default for ProcessingSemantics {
    fn default() -> Self {
        Self {
        limits:"Veoveo service admission bounds, not Google model capabilities".into(),
        preprocessing:"Native CPU decoding, resampling, image patches and video sampling".into(),
        dimensions:"Native Sentence Transformers truncation followed by normalization".into(),
        video_timestamps:"Native prompt uses frame index/source FPS; coverage also records actual decoded PTS".into(),
        audio_from_video:"Native soundtrack append; incompatible with separate audio parts".into(),
        image_frames:"Native first image frame only".into(), text_placeholders:"Reserved media markers are refused".into(),
    }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, veoveo_hub_sdk::SingleOutput)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub model: ModelIdentity,
    pub dimensions: Dimensions,
    pub supported_dimensions: Vec<Dimensions>,
    pub normalized: bool,
    pub modalities: Vec<Modality>,
    pub prompt_names: Vec<PromptName>,
    pub default_prompt: String,
    pub defaults: Preprocessing,
    pub execution: CapabilityExecution,
    pub runtime: RuntimeIdentity,
    pub limits: Limits,
    pub processing_semantics: ProcessingSemantics,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveProcessing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_name: Option<PromptName>,
    pub prompt_sha256: Sha256Digest,
    pub preprocessing: Preprocessing,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaSource {
    pub artifact_id: ArtifactId,
    pub media_type: MediaType,
    pub size: ObjectSize,
    pub sha256: Sha256Digest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageCoverage {
    pub width: u64,
    pub height: u64,
    pub frame_index: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioCoverage {
    pub source_duration_seconds: f64,
    pub source_sampling_rate: u64,
    pub source_channels: u64,
    pub sampling_rate: u64,
    pub decoded_samples: u64,
    pub retained_samples: u64,
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoCoverage {
    pub source_duration_seconds: f64,
    pub source_frame_count: u64,
    pub source_fps: f64,
    pub width: u64,
    pub height: u64,
    pub frame_indices: Vec<u64>,
    pub frame_timestamps_seconds: Vec<f64>,
    pub model_frame_timestamps_seconds: Vec<f64>,
    pub audio_included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioCoverage>,
}
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum MediaCoverage {
    Image {
        #[serde(rename = "partIndex")]
        part_index: usize,
        source: MediaSource,
        coverage: ImageCoverage,
    },
    Audio {
        #[serde(rename = "partIndex")]
        part_index: usize,
        source: MediaSource,
        coverage: AudioCoverage,
    },
    Video {
        #[serde(rename = "partIndex")]
        part_index: usize,
        source: MediaSource,
        coverage: VideoCoverage,
    },
}
// Preserve native JSON decimal decoding. Serde's internally tagged buffering
// cannot carry serde_json's arbitrary_precision number representation into f64.
// Capture only the coverage payload, then decode the closed native struct directly.
impl<'de> Deserialize<'de> for MediaCoverage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "type")]
            modality: Modality,
            part_index: usize,
            source: MediaSource,
            coverage: Box<serde_json::value::RawValue>,
        }
        let Wire {
            modality,
            part_index,
            source,
            coverage,
        } = Wire::deserialize(deserializer)?;
        match modality {
            Modality::Image => serde_json::from_str(coverage.get()).map(|coverage| Self::Image {
                part_index,
                source,
                coverage,
            }),
            Modality::Audio => serde_json::from_str(coverage.get()).map(|coverage| Self::Audio {
                part_index,
                source,
                coverage,
            }),
            Modality::Video => serde_json::from_str(coverage.get()).map(|coverage| Self::Video {
                part_index,
                source,
                coverage,
            }),
            Modality::Text => return Err(serde::de::Error::custom("text has no media coverage")),
        }
        .map_err(serde::de::Error::custom)
    }
}
impl MediaCoverage {
    pub fn part_index(&self) -> usize {
        match self {
            Self::Image { part_index, .. }
            | Self::Audio { part_index, .. }
            | Self::Video { part_index, .. } => *part_index,
        }
    }
    pub fn source(&self) -> &MediaSource {
        match self {
            Self::Image { source, .. }
            | Self::Audio { source, .. }
            | Self::Video { source, .. } => source,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Embedding {
    pub index: usize,
    pub vector: Vec<f32>,
    pub token_count: u64,
    pub media: Vec<MediaCoverage>,
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
    pub processing_id: Sha256Digest,
    pub dimensions: Dimensions,
    pub normalized: bool,
    pub execution: Execution,
    pub runtime: RuntimeIdentity,
    pub processing: EffectiveProcessing,
    pub items: Vec<Embedding>,
    pub usage: TokenUsage,
}

macro_rules! rejection_codes {
    ($($variant:ident => $wire:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        pub enum RejectionCode { $(#[serde(rename=$wire)] $variant),+ }
        impl std::fmt::Display for RejectionCode { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(match self { $(Self::$variant => $wire),+ }) } }
        impl std::error::Error for RejectionCode {}
    }
}
rejection_codes! {
    InvalidRequest=>"invalid_request", InvalidBatch=>"invalid_batch", InvalidItem=>"invalid_item", InvalidPart=>"invalid_part",
    EmptyContent=>"empty_content", InvalidUnicode=>"invalid_unicode", InvalidDimensions=>"invalid_dimensions", InvalidPrompt=>"invalid_prompt",
    InvalidOption=>"invalid_option", InvalidMediaType=>"invalid_media_type", InvalidSource=>"invalid_source", ReservedPlaceholder=>"reserved_placeholder",
    IncompatibleAudioOptions=>"incompatible_audio_options", ContentLimit=>"content_limit", ItemTokenLimit=>"item_token_limit", BatchTokenLimit=>"batch_token_limit",
    RequestSize=>"request_size", InvalidJson=>"invalid_json", Busy=>"busy", Unauthorized=>"unauthorized", NotReady=>"not_ready",
    SourceOrigin=>"source_origin", SourceExpired=>"source_expired", SourceTimeout=>"source_timeout", SourceRedirect=>"source_redirect", SourceRefused=>"source_refused",
    SourceEncoding=>"source_encoding", SourceSize=>"source_size", SourceDigest=>"source_digest", SourceUnavailable=>"source_unavailable",
    MediaDuration=>"media_duration", MediaPixels=>"media_pixels", MediaMetadata=>"media_metadata", MissingAudioStream=>"missing_audio_stream", InvalidMedia=>"invalid_media",
    PreprocessingCardinality=>"preprocessing_cardinality", PreprocessingCoverage=>"preprocessing_coverage", PreprocessingFailed=>"preprocessing_failed",
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
        matches!(
            self,
            Self::InvalidRequest
                | Self::InvalidBatch
                | Self::InvalidItem
                | Self::InvalidPart
                | Self::EmptyContent
                | Self::InvalidUnicode
                | Self::InvalidDimensions
                | Self::InvalidPrompt
                | Self::InvalidOption
                | Self::InvalidMediaType
                | Self::ReservedPlaceholder
                | Self::IncompatibleAudioOptions
                | Self::ContentLimit
                | Self::ItemTokenLimit
                | Self::BatchTokenLimit
                | Self::RequestSize
                | Self::InvalidJson
                | Self::MediaDuration
                | Self::MediaPixels
                | Self::MissingAudioStream
                | Self::InvalidMedia
        )
    }
}
