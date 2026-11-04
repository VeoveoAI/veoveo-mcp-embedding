//! Private runtime wire types. A media part's signed GET is the caller's credential: it is
//! never part of public output, and every type here redacts it in Debug output.
use crate::contract::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use veoveo_types::Sha256Digest;

/// The caller's signed GET of one media part: the URL and the headers its signature binds,
/// which the runtime sends unchanged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedGet {
    pub url: SignedUrl,
    #[serde(default, skip_serializing_if = "MediaHeaders::is_empty")]
    pub headers: MediaHeaders,
}
impl std::fmt::Debug for SignedGet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignedGet([REDACTED])")
    }
}

/// One media part as the runtime fetches it: the stated media type and size and the
/// caller's signed GET. There is no identity of the media's holder.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSource {
    pub media_type: MediaType,
    pub size: MediaSize,
    pub get: SignedGet,
}
impl RuntimeSource {
    pub fn of(media: &SignedMedia) -> Self {
        Self {
            media_type: media.media_type.clone(),
            size: media.size,
            get: SignedGet {
                url: media.url.clone(),
                headers: media.headers.clone(),
            },
        }
    }
    /// What the part states, without its signed GET.
    pub fn identity(&self) -> MediaSource {
        MediaSource {
            media_type: self.media_type.clone(),
            size: self.size,
        }
    }
}
impl std::fmt::Debug for RuntimeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSource")
            .field("media_type", &self.media_type)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum RuntimePart {
    Text { text: String },
    Image { source: RuntimeSource },
    Audio { source: RuntimeSource },
    Video { source: RuntimeSource },
}
impl RuntimePart {
    /// The runtime's form of a public part.
    pub fn of(part: &ContentPart) -> Self {
        match part {
            ContentPart::Text { text } => Self::Text { text: text.clone() },
            ContentPart::Image(media) => Self::Image {
                source: RuntimeSource::of(media),
            },
            ContentPart::Audio(media) => Self::Audio {
                source: RuntimeSource::of(media),
            },
            ContentPart::Video(media) => Self::Video {
                source: RuntimeSource::of(media),
            },
        }
    }
    pub fn source(&self) -> Option<&RuntimeSource> {
        match self {
            Self::Text { .. } => None,
            Self::Image { source } | Self::Audio { source } | Self::Video { source } => {
                Some(source)
            }
        }
    }
    pub fn modality(&self) -> Modality {
        match self {
            Self::Text { .. } => Modality::Text,
            Self::Image { .. } => Modality::Image,
            Self::Audio { .. } => Modality::Audio,
            Self::Video { .. } => Modality::Video,
        }
    }
    fn shape(&self) -> PartShape {
        match self {
            Self::Text { text } => PartShape::Text { text: text.clone() },
            Self::Image { source } => PartShape::Image(source.identity()),
            Self::Audio { source } => PartShape::Audio(source.identity()),
            Self::Video { source } => PartShape::Video(source.identity()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeItem {
    pub content: Vec<RuntimePart>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRequest {
    pub request_id: uuid::Uuid,
    pub items: Vec<RuntimeItem>,
    pub dimensions: Dimensions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_name: Option<PromptName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub preprocessing: Preprocessing,
}
impl std::fmt::Debug for RuntimeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeRequest")
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}
impl RuntimeRequest {
    /// The runtime's form of `input`: its parts in order, each media part with the
    /// caller's signed GET.
    pub fn new(request_id: uuid::Uuid, input: &EmbedRequest) -> Self {
        Self::from_items(
            request_id,
            input,
            input
                .items
                .iter()
                .map(|item| RuntimeItem {
                    content: item.content.iter().map(RuntimePart::of).collect(),
                })
                .collect(),
        )
    }
    /// `input`'s options with explicitly given runtime items.
    pub fn from_items(
        request_id: uuid::Uuid,
        input: &EmbedRequest,
        items: Vec<RuntimeItem>,
    ) -> Self {
        Self {
            request_id,
            items,
            dimensions: input.dimensions,
            prompt_name: input.prompt_name,
            prompt: input.prompt.clone(),
            preprocessing: input.preprocessing.effective(),
        }
    }
    /// The request as its result is validated against it, without signed GETs.
    pub fn shape(&self) -> RequestShape {
        RequestShape {
            items: self
                .items
                .iter()
                .map(|item| ShapeItem {
                    content: item.content.iter().map(RuntimePart::shape).collect(),
                })
                .collect(),
            dimensions: self.dimensions,
            prompt_name: self.prompt_name,
            prompt: self.prompt.clone(),
            preprocessing: self.preprocessing.clone(),
        }
    }
    pub fn effective_prompt(&self) -> &str {
        self.prompt
            .as_deref()
            .unwrap_or_else(|| self.prompt_name.map_or("", PromptName::prefix))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        self.shape().validate()?;
        if self.request_id.get_version_num() != 7 {
            return Err("request identity must be UUIDv7");
        }
        // Signed headers/URLs can expand far beyond public text bytes. Check the actual
        // private serialization before reserving inference usage.
        if serde_json::to_vec(self)
            .map_err(|_| "private payload serialization failed")?
            .len()
            > MAX_JSON_BYTES
        {
            return Err("private runtime payload exceeds 512 KiB");
        }
        Ok(())
    }
    pub fn processing_descriptor(&self) -> ProcessingDescriptor<'_> {
        descriptor(
            self.dimensions,
            self.effective_prompt(),
            &self.preprocessing,
        )
    }

    pub fn processing_id(&self) -> Result<Sha256Digest, serde_json::Error> {
        serde_json_canonicalizer::to_vec(&self.processing_descriptor()).map(|bytes| digest(&bytes))
    }
    pub fn effective_processing(&self) -> EffectiveProcessing {
        EffectiveProcessing {
            prompt_name: self.prompt_name,
            prompt_sha256: digest(self.effective_prompt().as_bytes()),
            preprocessing: self.preprocessing.effective(),
        }
    }
}
// Pure validation provenance also works with a saved request shape: no signed GET is
// needed, faked or kept for acceptance.
impl RequestShape {
    pub fn processing_descriptor(&self) -> ProcessingDescriptor<'_> {
        descriptor(
            self.dimensions,
            self.effective_prompt(),
            &self.preprocessing,
        )
    }
    pub fn processing_id(&self) -> Result<Sha256Digest, serde_json::Error> {
        serde_json_canonicalizer::to_vec(&self.processing_descriptor()).map(|bytes| digest(&bytes))
    }
    pub fn effective_processing(&self) -> EffectiveProcessing {
        EffectiveProcessing {
            prompt_name: self.prompt_name,
            prompt_sha256: digest(self.effective_prompt().as_bytes()),
            preprocessing: self.preprocessing.effective(),
        }
    }
}
impl EmbedRequest {
    pub fn processing_descriptor(&self) -> ProcessingDescriptor<'_> {
        descriptor(
            self.dimensions,
            self.effective_prompt(),
            &self.preprocessing,
        )
    }
    pub fn processing_id(&self) -> Result<Sha256Digest, serde_json::Error> {
        serde_json_canonicalizer::to_vec(&self.processing_descriptor()).map(|bytes| digest(&bytes))
    }
    pub fn effective_processing(&self) -> EffectiveProcessing {
        EffectiveProcessing {
            prompt_name: self.prompt_name,
            prompt_sha256: digest(self.effective_prompt().as_bytes()),
            preprocessing: self.preprocessing.effective(),
        }
    }
}
fn descriptor<'a>(
    dimensions: Dimensions,
    prompt: &'a str,
    preprocessing: &Preprocessing,
) -> ProcessingDescriptor<'a> {
    ProcessingDescriptor {
        model_id: MODEL_ID,
        model_revision: MODEL_REVISION,
        dimensions: dimensions.into(),
        normalized: true,
        pooling: "upstream_mean_projection_normalize",
        prompt,
        preprocessing: preprocessing.effective(),
        layout: "ordered_structured_conversations",
        audio_sampling_rate: 16000,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingDescriptor<'a> {
    pub model_id: &'a str,
    pub model_revision: &'a str,
    pub dimensions: usize,
    pub normalized: bool,
    pub pooling: &'a str,
    pub prompt: &'a str,
    pub preprocessing: Preprocessing,
    pub layout: &'a str,
    pub audio_sampling_rate: u64,
}
pub fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}
