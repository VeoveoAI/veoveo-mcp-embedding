//! Native registered caller -> Embedding -> typed Artifact calls. Runtime vectors,
//! media metadata and transfer bytes here are synthetic fixtures, not model evidence.
#![cfg(feature = "server")]
mod support;
use rmcp::{
    ErrorData,
    model::{ServerCapabilities, ServerConfig},
};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use support::forwarder::Forwarder;
use veoveo_hub_sdk::{
    HubInternalIdentity,
    docs::ServerDocs,
    hosting::{
        DomainServer, Hosted, RoutePolicy, Routes, hub_identity,
        testing::{self, TestMesh},
    },
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_mcp_artifact::contract::{
    self as artifact, ArtifactPermission, CreateDownloadRequest, CreateDownloadResult,
};
use veoveo_mcp_embedding::{
    contract::*, mcp::EmbeddingMcp, protocol::*, provider::RuntimeClient, usage::CoreMeter,
};
use veoveo_types::{ResourceScheme, ServerSlug};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum NoScope {}
fn docs(slug: &str) -> ServerDocs {
    ServerDocs::new(ServerSlug::parse(slug).unwrap())
        .with_doc(
            "agents",
            "Synthetic fixture",
            "# Native composition fixture",
        )
        .with_doc("design", "Design", "# Synthetic tests only")
}
fn config() -> ServerConfig {
    let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
    config.server_info = rmcp::model::Implementation::new("artifact", env!("CARGO_PKG_VERSION"));
    config
}
#[derive(Clone, Default)]
struct ArtifactFixture {
    mode: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<HubInternalIdentity>>>,
}
struct ArtifactContract;
static ARTIFACT_DOCS: LazyLock<ServerDocs> = LazyLock::new(|| docs("artifact"));
static ARTIFACT_SETUP: LazyLock<McpServerSetup<ArtifactContract>> =
    LazyLock::new(|| McpServerSetup::new().unwrap());
impl McpServerContract for ArtifactContract {
    type Server = ArtifactFixture;
    type Scope = NoScope;
    type Provider = artifact::ArtifactProvider;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("artifact").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        &[]
    }
    fn documents() -> &'static ServerDocs {
        &ARTIFACT_DOCS
    }
    fn server_config() -> ServerConfig {
        config()
    }
}
static ARTIFACT_ROUTES: LazyLock<Routes<ArtifactFixture>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<CreateDownloadRequest, _, _>(
            "Synthetic typed Artifact grant",
            RoutePolicy::permission(ArtifactPermission::ArtifactsRead).unwrap(),
            |domain: ArtifactFixture, request, context| async move {
                domain.seen.lock().unwrap().push(hub_identity(&context)?);
                if domain.mode.load(Ordering::SeqCst) == 1 {
                    return Err(
                        ErrorData::invalid_request("synthetic-private-ACL-reason", None).into(),
                    );
                }
                let mut answer = download(request.artifact_id);
                match domain.mode.load(Ordering::SeqCst) {
                    2 => {
                        answer.artifact.artifact_id =
                            ArtifactId::parse(uuid::Uuid::now_v7().to_string()).unwrap()
                    }
                    3 => answer.artifact.size = None,
                    4 => answer.artifact.sha256 = None,
                    5 => answer.artifact.state = artifact::ArtifactState::Verifying,
                    6 => {
                        answer.artifact.media_type =
                            artifact::MediaType::parse("application/octet-stream").unwrap()
                    }
                    7 => answer.get.method = artifact::TransferMethod::Put,
                    8 => answer.get.expires_at = chrono::Utc::now() - chrono::TimeDelta::seconds(1),
                    9 => answer.artifact.size = Some(artifact::ObjectSize::new(0).unwrap()),
                    10 => {
                        answer.artifact.size =
                            Some(artifact::ObjectSize::new(MAX_ARTIFACT_BYTES + 1).unwrap())
                    }
                    11 => {
                        answer.artifact.size =
                            Some(artifact::ObjectSize::new(MAX_ARTIFACT_BYTES).unwrap())
                    }
                    12 => {
                        answer.get.headers = (0..32)
                            .map(|index| (format!("X-{index}"), "x".repeat(8192)))
                            .collect();
                    }
                    _ => {}
                }
                Ok(answer)
            },
        )
        .build()
});
impl DomainServer for ArtifactFixture {
    type Contract = ArtifactContract;
    fn setup() -> &'static McpServerSetup<ArtifactContract> {
        &ARTIFACT_SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ARTIFACT_ROUTES
    }
}
fn download(id: ArtifactId) -> CreateDownloadResult {
    let principal = testing::principal();
    CreateDownloadResult {
        artifact: artifact::ArtifactRecord {
            artifact_id: id,
            name: artifact::ArtifactName::parse("synthetic.png").unwrap(),
            media_type: artifact::MediaType::parse("image/png").unwrap(),
            size: Some(artifact::ObjectSize::new(3).unwrap()),
            sha256: Some(veoveo_types::Sha256Digest::from_bytes([0; 32])),
            owner: principal.id.clone(),
            work_context: testing::authority().work_context,
            labels: Default::default(),
            release_state: artifact::ReleaseState::Private,
            state: artifact::ArtifactState::Available,
            provenance: artifact::ArtifactProvenance {
                caller: principal.id,
                acting_server: None,
                task: None,
                derived_from: Default::default(),
            },
            metadata: Default::default(),
            created_at: chrono::Utc::now(),
            available_at: Some(chrono::Utc::now()),
            retain_until: None,
        },
        get: artifact::SignedTransfer {
            method: artifact::TransferMethod::Get,
            url: artifact::SignedUrl::parse(
                "http://private.test:9000/object?synthetic-secret=never-public",
            )
            .unwrap(),
            headers: std::collections::BTreeMap::from([(
                "X-Synthetic-Signature".into(),
                "private-header".into(),
            )]),
            expires_at: chrono::Utc::now() + chrono::TimeDelta::minutes(5),
        },
    }
}
async fn fake_runtime() -> (
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
                let mut result = support::output(&request);
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
#[tokio::test]
async fn typed_artifact_resolution_is_caller_bound_direct_and_composed_before_inference_accounting()
{
    tokio::time::timeout(std::time::Duration::from_secs(60),async {
        let _=rustls::crypto::ring::default_provider().install_default();
        let db=veoveo_fixtures::store::TestDb::new().await;let store=db.server_store(&veoveo_mcp_embedding::server()).await;
        let (origin,runtime_seen,runtime_task)=fake_runtime().await;
        let runtime=RuntimeClient::new(origin,secrecy::SecretString::from("synthetic-bearer"),Device::Mps).unwrap();
        let domain=EmbeddingMcp::new(runtime,CoreMeter::new(store.clone()).await.unwrap(),support::capabilities());
        let denied_domain=domain.clone();
        let artifact=ArtifactFixture::default();let mut mesh=TestMesh::bind().await;
        mesh.register(ARTIFACT_ROUTES.authorization());let artifact_copy=artifact.clone();
        mesh.serve(testing::for_domain::<ArtifactFixture>().handler(move||Hosted::new(artifact_copy.clone())).build());
        let calls=mesh.register(EmbeddingMcp::routes().authorization());
        mesh.serve(testing::for_domain::<EmbeddingMcp>().calls(calls).handler(move||Hosted::new(domain.clone())).build());
        let calls=mesh.register(Forwarder::routes().authorization());
        mesh.serve(testing::for_domain::<Forwarder>().calls(calls).handler(||Hosted::new(Forwarder)).build());
        let mesh=mesh.granting([ArtifactPermission::ArtifactsRead.name(),EmbeddingPermission::ModelsRead.name(),EmbeddingPermission::EmbeddingsCreate.name(),veoveo_types::PermissionName::parse("embeddingprobe.calls.make").unwrap()]).start();
        let image=||serde_json::json!({"type":"image","artifactId":uuid::Uuid::now_v7()});
        let request=serde_json::json!({"items":[{"content":[{"type":"text","text":"independent"}]},{"content":[{"type":"text","text":"before"},image(),{"type":"text","text":"between"},image()]}],"dimensions":256});
        // Native permission refusal hides Artifact before its handler runs.
        let mut denied=TestMesh::bind().await;
        denied.register(ARTIFACT_ROUTES.authorization());let denied_artifact=artifact.clone();
        denied.serve(testing::for_domain::<ArtifactFixture>().handler(move||Hosted::new(denied_artifact.clone())).build());
        let calls=denied.register(EmbeddingMcp::routes().authorization());
        denied.serve(testing::for_domain::<EmbeddingMcp>().calls(calls).handler(move||Hosted::new(denied_domain.clone())).build());
        let denied=denied.granting([EmbeddingPermission::ModelsRead.name(),EmbeddingPermission::EmbeddingsCreate.name()]).start();
        let refusal=denied.call("embedding","embed",request.clone()).await;
        assert!(refusal["error"].is_object()||refusal["result"]["isError"]==true);
        assert!(artifact.seen.lock().unwrap().is_empty());assert!(runtime_seen.lock().unwrap().is_empty());
        // Bad ACL/availability/integrity metadata never enters the GPU runtime or ledger.
        for mode in (1..=10).chain([12]) {artifact.mode.store(mode,Ordering::SeqCst);let answer=mesh.call("embedding","embed",request.clone()).await;
            assert!(answer["error"].is_object()||answer["result"]["isError"]==true,"mode{mode}");assert!(!answer.to_string().contains("synthetic-private-ACL-reason"));
            assert!(runtime_seen.lock().unwrap().is_empty());}
        let today=chrono::Utc::now().date_naive();
        assert!(veoveo_usage::UsageStore::new(store.clone()).daily(&testing::authority().tenant,today,today.succ_opt().unwrap(),20).await.unwrap().is_empty());
        artifact.mode.store(11,Ordering::SeqCst);
        let too_large=serde_json::json!({"items":[{"content":[image(),image(),image()]}]});
        let answer=mesh.call("embedding","embed",too_large).await;assert!(answer["result"]["isError"]==true||answer["error"].is_object());assert!(runtime_seen.lock().unwrap().is_empty());
        artifact.mode.store(0,Ordering::SeqCst);
        for composed in [false,true] {let answer=if composed {mesh.call("embeddingprobe","forward_embed",serde_json::json!({"request":request})).await}else{mesh.call("embedding","embed",request.clone()).await};
            assert_ne!(answer["result"]["isError"],true,"{answer}");let result:EmbeddingResult=serde_json::from_value(answer["result"]["structuredContent"].clone()).unwrap();
            assert_eq!(result.items.len(),2);assert_eq!(result.dimensions,Dimensions::D256);assert_eq!(result.items[1].media.len(),2);
            let seen=artifact.seen.lock().unwrap();let identity=seen.last().unwrap();assert_eq!(identity.acting_server().unwrap().as_str(),"embedding");assert_eq!(identity.client.as_str(),testing::TEST_CLIENT);
            assert_eq!(identity.delegation_context.as_ref().map_or(0,|c|c.actors.len()),usize::from(composed));
            if composed {assert_eq!(identity.delegation_context.as_ref().unwrap().actors[0].server.as_str(),"embeddingprobe");}
            assert!(!serde_json::to_string(&result).unwrap().contains("synthetic-secret"));
        }
        let sent=runtime_seen.lock().unwrap();assert_eq!(sent.len(),2);assert_eq!(sent[0].items[1].content.len(),4);assert_eq!(sent[0].items[1].content[1].source().unwrap().get.headers["X-Synthetic-Signature"],"private-header");
        runtime_task.abort();
    }).await.expect("bounded composition fixture exceeded60s");
}

#[test]
fn native_artifact_metadata_allows_audio_from_video_container_without_widening_other_modalities() {
    let artifact = ArtifactFixture::setup();
    assert_eq!(artifact.server_config().server_info.name, "artifact");
    assert_eq!(artifact.documents().server(), "artifact");
    let caller = Forwarder::setup();
    assert_eq!(caller.server_config().server_info.name, "embeddingprobe");
    assert_eq!(caller.documents().server(), "embeddingprobe");

    let id = ArtifactId::parse(uuid::Uuid::now_v7().to_string()).unwrap();
    let mut answer = download(id);
    answer.artifact.media_type = artifact::MediaType::parse("video/mp4").unwrap();
    assert!(
        veoveo_mcp_embedding::resolution::admitted_source(id, Modality::Audio, answer.clone())
            .is_ok()
    );
    assert!(
        veoveo_mcp_embedding::resolution::admitted_source(id, Modality::Video, answer.clone())
            .is_ok()
    );
    assert!(
        veoveo_mcp_embedding::resolution::admitted_source(id, Modality::Image, answer).is_err()
    );
}
