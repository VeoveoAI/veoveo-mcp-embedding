//! Native caller-bound Artifact authorization; this component never fetches bytes.
use crate::{contract::*, protocol::*};
use chrono::Utc;
use rmcp::{RoleServer, service::RequestContext};
use veoveo_hub_sdk::hosting::ClientContext;
use veoveo_mcp_artifact::contract::{
    ArtifactState, CreateDownloadRequest, CreateDownloadResult, TransferMethod,
};

#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    #[error(
        "artifact authorization or metadata is unavailable; inference not started; no inference usage reserved"
    )]
    Unavailable,
    #[error(
        "artifact metadata or signed transfer violates embedding admission; inference not started; no inference usage reserved"
    )]
    Invalid,
    #[error(
        "artifact transfer bytes exceed service bounds; inference not started; no inference usage reserved"
    )]
    Size,
}
pub fn admitted_source(
    id: ArtifactId,
    modality: Modality,
    result: CreateDownloadResult,
) -> Result<RuntimeSource, ResolutionError> {
    let artifact = result.artifact;
    let size = artifact.size.ok_or(ResolutionError::Invalid)?;
    let sha256 = artifact.sha256.ok_or(ResolutionError::Invalid)?;
    let mime = artifact.media_type.as_str();
    let family = match modality {
        Modality::Image => mime.starts_with("image/"),
        Modality::Audio => mime.starts_with("audio/") || mime.starts_with("video/"),
        Modality::Video => mime.starts_with("video/"),
        Modality::Text => false,
    };
    if artifact.artifact_id != id
        || artifact.state != ArtifactState::Available
        || !family
        || size.bytes() == 0
        || size.bytes() > MAX_ARTIFACT_BYTES
        || result.get.method != TransferMethod::Get
        || result.get.expires_at <= Utc::now()
    {
        return Err(ResolutionError::Invalid);
    }
    Ok(RuntimeSource {
        artifact_id: id,
        media_type: artifact.media_type,
        size,
        sha256,
        get: result.get,
    })
}
pub async fn resolve(
    request_id: uuid::Uuid,
    request: &EmbedRequest,
    context: &RequestContext<RoleServer>,
) -> Result<RuntimeRequest, ResolutionError> {
    let mut items = Vec::with_capacity(request.items.len());
    let mut total = 0_u64;
    // Binding a native client requires configured ServerCalls only when there is media.
    let media = request
        .items
        .iter()
        .flat_map(|item| &item.content)
        .any(|part| part.artifact_id().is_some());
    let client = media
        .then(|| context.client())
        .transpose()
        .map_err(|_| ResolutionError::Unavailable)?;
    for item in &request.items {
        let mut content = Vec::with_capacity(item.content.len());
        for part in &item.content {
            content.push(match part {
                ContentPart::Text { text } => RuntimePart::Text { text: text.clone() },
                _ => {
                    let id = part.artifact_id().ok_or(ResolutionError::Invalid)?;
                    let answer = client
                        .as_ref()
                        .ok_or(ResolutionError::Unavailable)?
                        .call(CreateDownloadRequest { artifact_id: id })
                        .await
                        .map_err(|_| ResolutionError::Unavailable)?;
                    let source = admitted_source(id, part.modality(), answer)?;
                    total = total
                        .checked_add(source.size.bytes())
                        .ok_or(ResolutionError::Size)?;
                    if total > MAX_TRANSFER_BYTES {
                        return Err(ResolutionError::Size);
                    }
                    match part {
                        ContentPart::Image { .. } => RuntimePart::Image { source },
                        ContentPart::Audio { .. } => RuntimePart::Audio { source },
                        ContentPart::Video { .. } => RuntimePart::Video { source },
                        ContentPart::Text { .. } => return Err(ResolutionError::Invalid),
                    }
                }
            });
        }
        items.push(RuntimeItem { content });
    }
    Ok(RuntimeRequest::new(request_id, request, items))
}
