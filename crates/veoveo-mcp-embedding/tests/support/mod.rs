//! Explicitly synthetic boundary fixtures. No model inference or valid media bytes.
#![allow(dead_code, reason = "Each component test uses a subset of the fixture")]
use std::sync::{Arc, Mutex};
use veoveo_mcp_embedding::{contract::*, protocol::*};
pub fn capabilities() -> Capabilities {
    serde_json::from_str(include_str!("../fixtures/synthetic-capabilities-mps.json")).unwrap()
}
pub fn input() -> EmbedRequest {
    serde_json::from_value(serde_json::json!({"items":[{"content":[{"type":"text","text":"synthetic private text"}]}]})).unwrap()
}
pub fn text_request(input: &EmbedRequest) -> RuntimeRequest {
    assert!(
        input
            .items
            .iter()
            .flat_map(|item| &item.content)
            .all(|part| part.signed_media().is_none()),
        "text fixture only"
    );
    RuntimeRequest::new(uuid::Uuid::now_v7(), input)
}
/// A synthetic signed URL: nothing serves it, and nothing may echo it.
pub const SIGNED_URL: &str = "http://private.test:9000/object?X-Amz-Signature=synthetic-secret";
/// A synthetic signed header value, which nothing may echo either.
pub const SIGNED_HEADER: &str = "synthetic-session-token";
/// Synthetic signed media of `media_type` and `size` bytes: no object stands behind it.
pub fn signed(media_type: &str, size: u64) -> SignedMedia {
    SignedMedia {
        url: SignedUrl::parse(SIGNED_URL).unwrap(),
        headers: MediaHeaders::new([(
            HeaderName::parse("x-amz-security-token").unwrap(),
            HeaderValue::parse(SIGNED_HEADER).unwrap(),
        )])
        .unwrap(),
        media_type: MediaType::parse(media_type).unwrap(),
        size: MediaSize::new(size).unwrap(),
    }
}
/// A synthetic media part of `modality`: `image/png`, `audio/wav` or `video/mp4`.
pub fn media(modality: Modality, size: u64) -> ContentPart {
    let media_type = match modality {
        Modality::Image => "image/png",
        Modality::Audio => "audio/wav",
        Modality::Video => "video/mp4",
        Modality::Text => panic!("a media part"),
    };
    ContentPart::media(modality, signed(media_type, size)).unwrap()
}
/// The JSON of a synthetic media part.
pub fn media_json(kind: &str, media_type: &str, size: u64) -> serde_json::Value {
    serde_json::json!({
        "type": kind,
        "url": SIGNED_URL,
        "headers": {"x-amz-security-token": SIGNED_HEADER},
        "mediaType": media_type,
        "size": size
    })
}
/// One request item of `parts`.
pub fn item(parts: Vec<ContentPart>) -> EmbeddingInput {
    EmbeddingInput {
        content: ContentParts::new(parts).unwrap(),
    }
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

/// A synthetic runtime on a loopback port: it records each request and answers it with a
/// unit vector per item and image coverage per media part, by position. No inference.
pub async fn runtime() -> (
    url::Url,
    Arc<Mutex<Vec<RuntimeRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let capture = seen.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = vec![];
            let mut buf = [0; 8192];
            let body = loop {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    break None;
                }
                raw.extend_from_slice(&buf[..n]);
                assert!(raw.len() < MAX_JSON_BYTES + 8192);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = std::str::from_utf8(&raw[..end]).unwrap();
                    let size: usize = header
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .unwrap()
                        .1
                        .trim()
                        .parse()
                        .unwrap();
                    if raw.len() >= end + 4 + size {
                        break Some(raw[end + 4..end + 4 + size].to_vec());
                    }
                }
            };
            if let Some(body) = body {
                let request: RuntimeRequest = serde_json::from_slice(&body).unwrap();
                let mut result = output(&request);
                for (item, input) in result.items.iter_mut().zip(&request.items) {
                    item.media = input
                        .content
                        .iter()
                        .enumerate()
                        .filter_map(|(part_index, part)| {
                            part.source().map(|source| MediaCoverage::Image {
                                part_index,
                                source: source.identity(),
                                coverage: ImageCoverage {
                                    width: 2,
                                    height: 2,
                                    frame_index: 0,
                                },
                            })
                        })
                        .collect();
                }
                capture.lock().unwrap().push(request);
                let body = serde_json::to_vec(&result).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
        }
    });
    (origin, seen, task)
}

pub mod forwarder;
