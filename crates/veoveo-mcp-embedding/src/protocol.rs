//! Private runtime wire types. Signed transfers are never part of public discovery.
use crate::contract::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use veoveo_mcp_artifact::contract::SignedTransfer;
use veoveo_types::Sha256Digest;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSource {
    pub artifact_id: ArtifactId,
    pub media_type: veoveo_mcp_artifact::contract::MediaType,
    pub size: veoveo_mcp_artifact::contract::ObjectSize,
    pub sha256: Sha256Digest,
    pub get: SignedTransfer,
}
impl RuntimeSource {
    pub fn identity(&self) -> MediaSource {
        MediaSource {
            artifact_id: self.artifact_id,
            media_type: self.media_type.clone(),
            size: self.size,
            sha256: self.sha256.clone(),
        }
    }
}
impl std::fmt::Debug for RuntimeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSource")
            .field("artifact_id", &self.artifact_id)
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
    fn public(&self) -> ContentPart {
        match self {
            Self::Text { text } => ContentPart::Text { text: text.clone() },
            Self::Image { source } => ContentPart::Image {
                artifact_id: source.artifact_id,
            },
            Self::Audio { source } => ContentPart::Audio {
                artifact_id: source.artifact_id,
            },
            Self::Video { source } => ContentPart::Video {
                artifact_id: source.artifact_id,
            },
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
    pub fn new(request_id: uuid::Uuid, input: &EmbedRequest, items: Vec<RuntimeItem>) -> Self {
        Self {
            request_id,
            items,
            dimensions: input.dimensions,
            prompt_name: input.prompt_name,
            prompt: input.prompt.clone(),
            preprocessing: input.preprocessing.effective(),
        }
    }
    pub fn public(&self) -> EmbedRequest {
        EmbedRequest {
            items: self
                .items
                .iter()
                .map(|item| EmbeddingInput {
                    content: item.content.iter().map(RuntimePart::public).collect(),
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
        self.public().validate()?;
        if self.request_id.get_version_num() != 7 {
            return Err("request identity must be UUIDv7");
        }
        let mut bytes = 0_u64;
        for source in self
            .items
            .iter()
            .flat_map(|item| &item.content)
            .filter_map(RuntimePart::source)
        {
            if source.size.bytes() == 0 || source.size.bytes() > MAX_ARTIFACT_BYTES {
                return Err("artifact size exceeds service bounds");
            }
            bytes = bytes
                .checked_add(source.size.bytes())
                .ok_or("artifact size overflow")?;
        }
        if bytes > MAX_TRANSFER_BYTES {
            return Err("aggregate artifact bytes exceed service bounds");
        }
        // Authorized signed headers/URLs can expand far beyond public input bytes.
        // Check the actual private serialization before reserving inference usage.
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
// Pure validation provenance also works with saved public inputs and authorized
// safe source identities: no fake/persisted transfer is needed for acceptance.
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
