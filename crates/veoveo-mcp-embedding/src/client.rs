//! The SDK's knowledge [`Embedder`] over this server's typed `embed` tool. A knowledge
//! source passes [`EmbeddingClient`] with the caller-bound `HubClient` its handler or Task
//! already holds, so the caller is the one who embeds and the usage is theirs.
use veoveo_hub_sdk::{
    client::{CallError, HubClient},
    knowledge::{
        ContentParts, Embedder, EmbeddingDimensions, EmbeddingModel, EmbeddingPurpose, Embeddings,
        ModelCheckpoint,
    },
};

use crate::contract::{
    Dimensions, EmbedRequest, EmbeddingInput, EmbeddingResult, Preprocessing, PromptName,
};

/// Embeds items through Embedding's `embed` tool: `Document` items with promptName
/// `Document`, `Query` items with `SearchQuery`, both at 768 dimensions with default
/// preprocessing. The items must fit one request's bounds.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddingClient;

/// Why items were not embedded.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The items do not fit one embed request; nothing was sent.
    #[error("the items do not fit one embed request: {0}")]
    Request(&'static str),
    /// The call failed; the error never holds a signed URL.
    #[error("the embed call failed: {0}")]
    Call(#[from] CallError),
    /// The result does not answer the items in order with the model's dimensions.
    #[error("the embed result does not answer its items")]
    Result,
}

impl EmbeddingClient {
    /// The request `embed` sends for `items` with `purpose`.
    pub fn request(items: &[ContentParts], purpose: EmbeddingPurpose) -> EmbedRequest {
        EmbedRequest {
            items: items
                .iter()
                .map(|content| EmbeddingInput {
                    content: content.clone(),
                })
                .collect(),
            dimensions: Dimensions::D768,
            prompt_name: Some(match purpose {
                EmbeddingPurpose::Document => PromptName::Document,
                EmbeddingPurpose::Query => PromptName::SearchQuery,
            }),
            prompt: None,
            preprocessing: Preprocessing::default(),
        }
    }

    /// The embeddings `result` gives `count` items: one vector per item in order, with the
    /// checkpoint `{id}@{revision}` and the dimensions the result reports.
    pub fn embeddings(result: EmbeddingResult, count: usize) -> Result<Embeddings, EmbedError> {
        if result.items.len() != count
            || result
                .items
                .iter()
                .enumerate()
                .any(|(index, item)| item.index != index)
        {
            return Err(EmbedError::Result);
        }
        let checkpoint =
            ModelCheckpoint::parse(format!("{}@{}", result.model.id, result.model.revision))
                .map_err(|_| EmbedError::Result)?;
        let dimensions = u32::try_from(usize::from(result.dimensions))
            .ok()
            .and_then(EmbeddingDimensions::new)
            .ok_or(EmbedError::Result)?;
        Embeddings::new(
            EmbeddingModel {
                checkpoint,
                dimensions,
            },
            result.items.into_iter().map(|item| item.vector).collect(),
        )
        .map_err(|_| EmbedError::Result)
    }
}

impl Embedder for EmbeddingClient {
    type Error = EmbedError;

    async fn embed(
        &self,
        client: &HubClient,
        items: &[ContentParts],
        purpose: EmbeddingPurpose,
    ) -> Result<Embeddings, EmbedError> {
        let request = Self::request(items, purpose);
        request.validate().map_err(EmbedError::Request)?;
        let result = client.call(request).await?;
        Self::embeddings(result, items.len())
    }
}
