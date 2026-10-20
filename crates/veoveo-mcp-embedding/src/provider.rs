//! Private runtime client. No automatic replay, dynamic destinations or content logs.
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, de::DeserializeOwned};
use url::Url;

pub use crate::validation::{validate_capabilities, validate_public_result, validate_result};
use crate::{contract::*, protocol::RuntimeRequest};

#[derive(Clone)]
pub struct RuntimeClient {
    http: reqwest::Client,
    origin: Url,
    credential: SecretString,
    device: Device,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime rejected the request before inference: {0}")]
    NotStarted(RejectionCode),
    #[error("runtime execution could not be confirmed")]
    Ambiguous,
    #[error("runtime response violates the fixed embedding contract")]
    InvalidResponse,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rejection {
    title: RejectionCode,
    execution: String,
    request_id: Option<uuid::Uuid>,
}

// Closed diagnostics only: never retain or format a reqwest error (which can
// contain its authenticated destination), response body, headers or caller input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Capabilities,
    Ready,
    Embed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Send,
    Status,
    RejectionProvenance,
    ResponseLength,
    Body,
    Json,
    Contract,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TransportFlags {
    connect: bool,
    timeout: bool,
    request: bool,
    body: bool,
    decode: bool,
    redirect: bool,
    status: bool,
}

impl TransportFlags {
    fn classify(error: &reqwest::Error) -> Self {
        Self {
            connect: error.is_connect(),
            timeout: error.is_timeout(),
            request: error.is_request(),
            body: error.is_body(),
            decode: error.is_decode(),
            redirect: error.is_redirect(),
            status: error.is_status(),
        }
    }
}

fn diagnose(
    operation: Operation,
    stage: Stage,
    http_status: Option<u16>,
    error: Option<&reqwest::Error>,
) {
    let flags = error.map(TransportFlags::classify).unwrap_or_default();
    tracing::warn!(
        operation = ?operation,
        stage = ?stage,
        http_status,
        is_connect = flags.connect,
        is_timeout = flags.timeout,
        is_request = flags.request,
        is_body = flags.body,
        is_decode = flags.decode,
        is_redirect = flags.redirect,
        is_status = flags.status,
        "private runtime failure classification"
    );
}

impl RuntimeClient {
    pub fn expected_device(&self) -> &Device {
        &self.device
    }
    pub fn new(origin: Url, credential: SecretString, device: Device) -> anyhow::Result<Self> {
        anyhow::ensure!(
            matches!(origin.scheme(), "http" | "https")
                && origin.host().is_some()
                && origin.username().is_empty()
                && origin.password().is_none()
                && origin.query().is_none()
                && origin.fragment().is_none()
                && origin.path() == "/"
                && origin.port() != Some(0),
            "runtime endpoint must be one fixed HTTP(S) origin"
        );
        anyhow::ensure!(
            !credential.expose_secret().is_empty(),
            "runtime credential is required"
        );
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            // The owned runtime closes idle HTTP connections after five seconds.
            // Retire our side first, including through a mediated connection.
            .pool_idle_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(REQUEST_SECONDS))
            .build()?;
        Ok(Self {
            http,
            origin,
            credential,
            device,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.origin.join(path).expect("fixed runtime path"))
            .bearer_auth(self.credential.expose_secret())
    }

    pub async fn capabilities(&self) -> Result<Capabilities, RuntimeError> {
        let response = self
            .request(reqwest::Method::GET, "capabilities")
            .send()
            .await
            .map_err(|error| {
                diagnose(Operation::Capabilities, Stage::Send, None, Some(&error));
                RuntimeError::Ambiguous
            })?;
        if !response.status().is_success() {
            diagnose(
                Operation::Capabilities,
                Stage::Status,
                Some(response.status().as_u16()),
                None,
            );
            return Err(RuntimeError::Ambiguous);
        }
        let status = response.status().as_u16();
        let value: Capabilities = bounded_json(response, 16384, Operation::Capabilities).await?;
        validate_capabilities(&value, &self.device).inspect_err(|_| {
            diagnose(Operation::Capabilities, Stage::Contract, Some(status), None);
        })?;
        Ok(value)
    }

    pub async fn ready(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(3), async {
            self.capabilities().await?;
            let response = self
                .request(reqwest::Method::GET, "readyz")
                .send()
                .await
                .map_err(|error| {
                    diagnose(Operation::Ready, Stage::Send, None, Some(&error));
                    RuntimeError::Ambiguous
                })?;
            if response.status().is_success() {
                Ok(())
            } else {
                diagnose(
                    Operation::Ready,
                    Stage::Status,
                    Some(response.status().as_u16()),
                    None,
                );
                Err(RuntimeError::Ambiguous)
            }
        })
        .await
        .is_ok_and(|result| result.is_ok())
    }

    pub async fn embed(&self, request: &RuntimeRequest) -> Result<EmbeddingResult, RuntimeError> {
        let response = self
            .request(reqwest::Method::POST, "embeddings")
            .json(request)
            .send()
            .await
            .map_err(|error| {
                diagnose(Operation::Embed, Stage::Send, None, Some(&error));
                RuntimeError::Ambiguous
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let error = bounded_json::<Rejection>(response, 4096, Operation::Embed).await;
            if let Ok(error) = error
                && error.execution == "not_started"
                && error.title.status() == status.as_u16()
                && (error.request_id == Some(request.request_id)
                    || (error.title == RejectionCode::Unauthorized && error.request_id.is_none()))
            {
                return Err(RuntimeError::NotStarted(error.title));
            }
            diagnose(
                Operation::Embed,
                Stage::RejectionProvenance,
                Some(status.as_u16()),
                None,
            );
            return Err(RuntimeError::Ambiguous);
        }
        let status = response.status().as_u16();
        let value = bounded_json(response, MAX_RESPONSE_BYTES, Operation::Embed).await?;
        validate_result(&value, request, &self.device).inspect_err(|_| {
            diagnose(Operation::Embed, Stage::Contract, Some(status), None);
        })?;
        Ok(value)
    }
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
    limit: usize,
    operation: Operation,
) -> Result<T, RuntimeError> {
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        diagnose(operation, Stage::ResponseLength, Some(status), None);
        return Err(RuntimeError::InvalidResponse);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        diagnose(operation, Stage::Body, Some(status), Some(&error));
        RuntimeError::Ambiguous
    })? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            diagnose(operation, Stage::ResponseLength, Some(status), None);
            return Err(RuntimeError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        diagnose(operation, Stage::Json, Some(status), None);
        RuntimeError::InvalidResponse
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{RuntimeItem, RuntimePart};
    fn synthetic_request(text: &str) -> RuntimeRequest {
        let input: EmbedRequest = serde_json::from_value(
            serde_json::json!({"items":[{"content":[{"type":"text","text":text}]}]}),
        )
        .unwrap();
        RuntimeRequest::new(
            uuid::Uuid::now_v7(),
            &input,
            vec![RuntimeItem {
                content: vec![RuntimePart::Text { text: text.into() }],
            }],
        )
    }
    fn synthetic_output(request: &RuntimeRequest, capabilities: &Capabilities) -> EmbeddingResult {
        let mut vector = vec![0.0; usize::from(request.dimensions)];
        vector[0] = 1.0;
        EmbeddingResult {
            request_id: request.request_id,
            model: capabilities.model.clone(),
            processing_id: request.processing_id().unwrap(),
            dimensions: request.dimensions,
            normalized: true,
            execution: Execution {
                device: Device::Mps,
                dtype: "float32".into(),
            },
            runtime: capabilities.runtime.clone(),
            processing: request.effective_processing(),
            items: vec![Embedding {
                index: 0,
                vector,
                token_count: 1,
                media: vec![],
            }],
            usage: TokenUsage { input_tokens: 1 },
        }
    }
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tracing::instrument::WithSubscriber as _;

    #[derive(Clone, Default)]
    struct Diagnostics(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

    impl tracing::Subscriber for Diagnostics {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Fields(BTreeMap<String, String>);
            impl tracing::field::Visit for Fields {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    self.0.insert(field.name().into(), format!("{value:?}"));
                }
            }
            if event.metadata().target() == module_path!().trim_end_matches("::tests") {
                let mut fields = Fields(BTreeMap::new());
                event.record(&mut fields);
                self.0.lock().unwrap().push(fields.0);
            }
        }
    }

    #[tokio::test]
    async fn diagnostics_classify_transport_body_and_status_without_private_data() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        // All payloads/credentials below are synthetic; no model or native provider.
        let request = synthetic_request("synthetic-private-input");
        let capture = Diagnostics::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let client = RuntimeClient::new(
            origin,
            SecretString::from("synthetic-private-credential"),
            Device::Mps,
        )
        .unwrap();
        let result = client
            .embed(&request)
            .with_subscriber(capture.clone())
            .await;
        assert!(matches!(result, Err(RuntimeError::Ambiguous)));
        let send = capture.0.lock().unwrap().last().unwrap().clone();
        assert_eq!(send["operation"], "Embed");
        assert_eq!(send["stage"], "Send");
        assert_eq!(send["is_connect"], "true");
        assert_eq!(send["is_request"], "true");

        let (origin, task) = peer("200 OK", "", "synthetic-private-response", Some(1000)).await;
        let client = RuntimeClient::new(
            origin,
            SecretString::from("synthetic-private-credential"),
            Device::Mps,
        )
        .unwrap();
        let result = client
            .embed(&request)
            .with_subscriber(capture.clone())
            .await;
        assert!(matches!(result, Err(RuntimeError::Ambiguous)));
        task.await.unwrap();
        let body = capture.0.lock().unwrap().last().unwrap().clone();
        assert_eq!(body["stage"], "Body");
        assert_eq!(body["http_status"], "200");
        assert_eq!(body["is_decode"], "true");

        let (origin, task) = peer("502 Bad Gateway", "", "{}", None).await;
        let client = RuntimeClient::new(
            origin,
            SecretString::from("synthetic-private-credential"),
            Device::Mps,
        )
        .unwrap();
        let result = client
            .embed(&request)
            .with_subscriber(capture.clone())
            .await;
        assert!(matches!(result, Err(RuntimeError::Ambiguous)));
        task.await.unwrap();
        let rejection = capture.0.lock().unwrap().last().unwrap().clone();
        assert_eq!(rejection["stage"], "RejectionProvenance");
        assert_eq!(rejection["http_status"], "502");

        let allowed = [
            "message",
            "operation",
            "stage",
            "http_status",
            "is_connect",
            "is_timeout",
            "is_request",
            "is_body",
            "is_decode",
            "is_redirect",
            "is_status",
        ];
        for fields in capture.0.lock().unwrap().iter() {
            assert!(fields.keys().all(|name| allowed.contains(&name.as_str())));
            for value in fields.values() {
                assert!(!value.contains("synthetic-private"));
                assert!(!value.contains("http://"));
                assert!(!value.contains("127.0.0.1"));
            }
        }
    }

    #[tokio::test]
    async fn expired_mediated_connection_is_retired_before_one_delayed_post() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        // Synthetic intermediary and unit vector only; no provider or inference.
        // It leaves the downstream socket open after its upstream expires at5s.
        // A POST on that old connection fails without a response, like stale EOF.
        let capabilities: Capabilities = serde_json::from_str(include_str!(
            "../tests/fixtures/synthetic-capabilities-mps.json"
        ))
        .unwrap();
        let request = synthetic_request("synthetic");
        let output = synthetic_output(&request, &capabilities);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let cap_body = serde_json::to_string(&capabilities).unwrap();
        let body = serde_json::to_string(&output).unwrap();
        let task = tokio::spawn(async move {
            let (mut initial, _) = listener.accept().await.unwrap();
            let initial_request = read_http_request(&mut initial).await;
            assert!(initial_request.starts_with(b"GET /capabilities HTTP/1.1\r\n"));
            initial.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{cap_body}", cap_body.len()).as_bytes()).await.unwrap();
            initial.flush().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
            // Do not propagate upstream expiry to the old downstream socket.
            let (fresh, mut socket, bytes) = tokio::select! {
                accepted = listener.accept() => {
                    let (mut socket, _) = accepted.unwrap();
                    let bytes = read_http_request(&mut socket).await;
                    (true, socket, bytes)
                }
                bytes = read_http_request(&mut initial) => {
                    if bytes.is_empty() {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let bytes = read_http_request(&mut socket).await;
                        (true, socket, bytes)
                    } else {
                        (false, initial, bytes)
                    }
                }
            };
            let mut posts = usize::from(bytes.starts_with(b"POST /embeddings HTTP/1.1\r\n"));
            assert_eq!(posts, 1);
            if fresh {
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            socket.shutdown().await.unwrap();
            // Any automatic retry would require a third accepted connection.
            if let Ok(Ok((mut extra, _))) =
                tokio::time::timeout(Duration::from_millis(100), listener.accept()).await
            {
                let bytes = read_http_request(&mut extra).await;
                posts += usize::from(bytes.starts_with(b"POST /embeddings HTTP/1.1\r\n"));
            }
            (fresh, posts)
        });
        let client =
            RuntimeClient::new(origin, SecretString::from("synthetic"), Device::Mps).unwrap();
        client.capabilities().await.unwrap();
        tokio::time::sleep(Duration::from_secs(6)).await;
        let result = tokio::time::timeout(Duration::from_secs(2), client.embed(&request))
            .await
            .unwrap();
        let (fresh, posts) = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            fresh,
            "delayed POST reused the intermediary's expired upstream"
        );
        assert_eq!(posts, 1, "embedding was replayed");
        assert_eq!(result.unwrap().request_id, request.request_id);
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
                return bytes;
            }
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() <= MAX_JSON_BYTES + 8192);
            if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .map_or(0, |(_, value)| value.trim().parse().unwrap());
                if bytes.len() >= end + 4 + length {
                    return bytes;
                }
            }
        }
    }

    /// Synthetic HTTP peer only; it never performs inference or resolves a credential.
    async fn peer(
        status: &str,
        extra: &str,
        body: &str,
        claimed_length: Option<usize>,
    ) -> (Url, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let reply = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n{extra}\r\n{body}",
            claimed_length.unwrap_or(body.len())
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let bytes = read_http_request(&mut socket).await;
            socket.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (origin, task)
    }

    #[tokio::test]
    async fn private_http_boundary_never_follows_redirects_and_requires_explicit_execution_state() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cases = [
            (
                "307 Temporary Redirect",
                "location: http://not-an-admitted-host.invalid/embeddings\r\n",
                "{}",
                None,
                0,
            ),
            (
                "422 Unprocessable Content",
                "",
                "{\"message\":\"synthetic rejection without execution provenance\"}",
                None,
                0,
            ),
            (
                "401 Unauthorized",
                "",
                "{\"title\":\"unauthorized\",\"execution\":\"not_started\",\"requestId\":null}",
                None,
                1,
            ),
            ("200 OK", "", "{}", Some(MAX_RESPONSE_BYTES + 1), 2),
        ];
        for (status, extra, body, length, expected) in cases {
            let (origin, task) = peer(status, extra, body, length).await;
            let runtime = RuntimeClient::new(
                origin,
                SecretString::from("synthetic-opaque-reference"),
                Device::Mps,
            )
            .unwrap();
            let request = synthetic_request("synthetic text");
            let error = tokio::time::timeout(Duration::from_secs(2), runtime.embed(&request))
                .await
                .unwrap()
                .unwrap_err();
            match expected {
                0 => assert!(matches!(error, RuntimeError::Ambiguous)),
                1 => assert!(matches!(
                    error,
                    RuntimeError::NotStarted(RejectionCode::Unauthorized)
                )),
                _ => assert!(matches!(error, RuntimeError::InvalidResponse)),
            }
            let request = task.await.unwrap();
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-opaque-reference")
            );
        }
    }
    #[tokio::test]
    async fn rejection_reason_requires_closed_code_exact_status_execution_and_request_identity() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for (code, status, execution, wrong_id, accepted) in [
            (
                "item_token_limit",
                "422 Unprocessable Content",
                "not_started",
                false,
                true,
            ),
            (
                "batch_token_limit",
                "422 Unprocessable Content",
                "not_started",
                false,
                true,
            ),
            ("busy", "429 Too Many Requests", "not_started", false, true),
            (
                "not_ready",
                "503 Service Unavailable",
                "not_started",
                false,
                true,
            ),
            (
                "arbitrary private text",
                "422 Unprocessable Content",
                "not_started",
                false,
                false,
            ),
            (
                "busy",
                "503 Service Unavailable",
                "not_started",
                false,
                false,
            ),
            (
                "item_token_limit",
                "422 Unprocessable Content",
                "unknown",
                false,
                false,
            ),
            (
                "item_token_limit",
                "422 Unprocessable Content",
                "not_started",
                true,
                false,
            ),
        ] {
            let request = synthetic_request("synthetic");
            let id = if wrong_id {
                uuid::Uuid::now_v7()
            } else {
                request.request_id
            };
            let body =
                serde_json::json!({"title":code,"execution":execution,"requestId":id}).to_string();
            let (origin, task) = peer(status, "", &body, None).await;
            let client =
                RuntimeClient::new(origin, SecretString::from("synthetic"), Device::Mps).unwrap();
            let result = client.embed(&request).await;
            if accepted {
                match result.unwrap_err() {
                    RuntimeError::NotStarted(reason) => assert_eq!(reason.to_string(), code),
                    _ => panic!("closed rejection was lost"),
                }
            } else {
                assert!(matches!(result, Err(RuntimeError::Ambiguous)));
            }
            task.await.unwrap();
        }
    }
}
