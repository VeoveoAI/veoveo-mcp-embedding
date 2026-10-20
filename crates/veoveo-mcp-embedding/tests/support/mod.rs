//! Explicitly synthetic boundary fixtures. No model inference or valid media bytes.
#![allow(dead_code, reason = "Each component test uses a subset of the fixture")]
use veoveo_mcp_embedding::{contract::*, protocol::*};
pub fn capabilities() -> Capabilities {
    serde_json::from_str(include_str!("../fixtures/synthetic-capabilities-mps.json")).unwrap()
}
pub fn input() -> EmbedRequest {
    serde_json::from_value(serde_json::json!({"items":[{"content":[{"type":"text","text":"synthetic private text"}]}]})).unwrap()
}
pub fn text_request(input: &EmbedRequest) -> RuntimeRequest {
    RuntimeRequest::new(
        uuid::Uuid::now_v7(),
        input,
        input
            .items
            .iter()
            .map(|item| RuntimeItem {
                content: item
                    .content
                    .iter()
                    .map(|part| match part {
                        ContentPart::Text { text } => RuntimePart::Text { text: text.clone() },
                        _ => panic!("text fixture only"),
                    })
                    .collect(),
            })
            .collect(),
    )
}
pub fn request() -> RuntimeRequest {
    text_request(&input())
}
pub fn output(request: &RuntimeRequest) -> EmbeddingResult {
    let caps = capabilities();
    let mut vector = vec![0.0; usize::from(request.dimensions)];
    vector[0] = 1.0;
    EmbeddingResult {
        request_id: request.request_id,
        model: caps.model,
        processing_id: request.processing_id().unwrap(),
        dimensions: request.dimensions,
        normalized: true,
        execution: Execution {
            device: Device::Mps,
            dtype: "float32".into(),
        },
        runtime: caps.runtime,
        processing: request.effective_processing(),
        items: request
            .items
            .iter()
            .enumerate()
            .map(|(index, _)| Embedding {
                index,
                vector: vector.clone(),
                token_count: 12,
                media: vec![],
            })
            .collect(),
        usage: TokenUsage {
            input_tokens: 12 * request.items.len() as u64,
        },
    }
}

pub mod forwarder;
