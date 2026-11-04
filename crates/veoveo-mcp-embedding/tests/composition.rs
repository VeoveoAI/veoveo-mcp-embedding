//! A registered caller and a composed one call Embedding with signed media, which reaches
//! a synthetic runtime as the caller's signed GET. Runtime vectors, media metadata and
//! signed URLs here are synthetic fixtures, not model or storage evidence.
#![cfg(feature = "server")]
mod support;
use support::forwarder::Forwarder;
use veoveo_hub_sdk::hosting::{
    DomainServer, Hosted,
    testing::{self, RunningMesh, TestMesh},
};
use veoveo_mcp_embedding::{
    contract::*, mcp::EmbeddingMcp, provider::RuntimeClient, usage::CoreMeter,
};
/// A mesh with Embedding over the synthetic runtime at `origin` and the composed caller.
async fn mesh(origin: url::Url, store: veoveo_store::PlatformStore) -> RunningMesh {
    let runtime = RuntimeClient::new(
        origin,
        secrecy::SecretString::from("synthetic-bearer"),
        Device::Mps,
    )
    .unwrap();
    let domain = EmbeddingMcp::new(
        runtime,
        CoreMeter::new(store).await.unwrap(),
        support::capabilities(),
    );
    let mut mesh = TestMesh::bind().await;
    mesh.register(EmbeddingMcp::routes().authorization());
    mesh.serve(
        testing::for_domain::<EmbeddingMcp>()
            .handler(move || Hosted::new(domain.clone()))
            .build(),
    );
    let calls = mesh.register(Forwarder::routes().authorization());
    mesh.serve(
        testing::for_domain::<Forwarder>()
            .calls(calls)
            .handler(|| Hosted::new(Forwarder))
            .build(),
    );
    mesh.granting([
        EmbeddingPermission::ModelsRead.name(),
        EmbeddingPermission::EmbeddingsCreate.name(),
        veoveo_types::PermissionName::parse("embeddingprobe.calls.make").unwrap(),
    ])
    .start()
}

#[tokio::test]
async fn signed_media_reaches_the_runtime_direct_and_composed_with_coverage_by_position() {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = veoveo_fixtures::store::TestDb::new().await;
        let store = db.server_store(&veoveo_mcp_embedding::server()).await;
        let (origin, runtime_seen, runtime_task) = support::runtime().await;
        let mesh = mesh(origin, store).await;
        let request = serde_json::json!({"items":[
            {"content":[{"type":"text","text":"independent"}]},
            {"content":[{"type":"text","text":"before"},support::media_json("image","image/png",3),
                {"type":"text","text":"between"},support::media_json("image","image/webp",5)]}
        ],"dimensions":256});
        for composed in [false, true] {
            let answer = if composed {
                mesh.call(
                    "embeddingprobe",
                    "forward_embed",
                    serde_json::json!({"request":request}),
                )
                .await
            } else {
                mesh.call("embedding", "embed", request.clone()).await
            };
            assert_ne!(answer["result"]["isError"], true, "{answer}");
            let text = answer.to_string();
            assert!(!text.contains("synthetic-secret") && !text.contains(support::SIGNED_HEADER));
            let result: EmbeddingResult =
                serde_json::from_value(answer["result"]["structuredContent"].clone()).unwrap();
            assert_eq!(result.items.len(), 2);
            assert_eq!(result.dimensions, Dimensions::D256);
            assert!(result.items[0].media.is_empty());
            // Coverage names each media part by its position and what it states.
            let rows: Vec<_> = result.items[1]
                .media
                .iter()
                .map(|row| (row.part_index(), row.source().clone()))
                .collect();
            assert_eq!(
                rows,
                [
                    (
                        1,
                        MediaSource {
                            media_type: MediaType::parse("image/png").unwrap(),
                            size: MediaSize::new(3).unwrap()
                        }
                    ),
                    (
                        3,
                        MediaSource {
                            media_type: MediaType::parse("image/webp").unwrap(),
                            size: MediaSize::new(5).unwrap()
                        }
                    ),
                ]
            );
        }
        let sent = runtime_seen.lock().unwrap();
        assert_eq!(sent.len(), 2);
        for request in sent.iter() {
            assert_eq!(request.items[1].content.len(), 4);
            let source = request.items[1].content[1].source().unwrap();
            assert_eq!(source.get.url.expose(), support::SIGNED_URL);
            let headers: Vec<_> = source
                .get
                .headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.expose()))
                .collect();
            assert_eq!(headers, [("x-amz-security-token", support::SIGNED_HEADER)]);
            assert!(
                !serde_json::to_string(request)
                    .unwrap()
                    .contains("artifactId")
            );
        }
        runtime_task.abort();
    })
    .await
    .expect("bounded composition fixture exceeded 60s");
}

#[tokio::test]
async fn stated_size_and_media_type_are_refused_before_the_runtime_or_any_usage() {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = veoveo_fixtures::store::TestDb::new().await;
        let store = db.server_store(&veoveo_mcp_embedding::server()).await;
        let (origin, runtime_seen, runtime_task) = support::runtime().await;
        let mesh = mesh(origin, store.clone()).await;
        let one = |part: serde_json::Value| serde_json::json!({"items":[{"content":[part]}]});
        let mut ftp = support::media_json("image", "image/png", 3);
        ftp["url"] = serde_json::json!(support::SIGNED_URL.replacen("http", "ftp", 1));
        for request in [
            one(support::media_json(
                "image",
                "image/png",
                MAX_SOURCE_BYTES + 1,
            )),
            serde_json::json!({"items":[{"content":[
                support::media_json("video","video/mp4",MAX_SOURCE_BYTES),
                support::media_json("video","video/mp4",MAX_SOURCE_BYTES),
                support::media_json("video","video/mp4",1)]}]}),
            one(support::media_json("image", "video/mp4", 3)),
            one(support::media_json("video", "audio/wav", 3)),
            one(support::media_json("image", "image/png", 0)),
            one(ftp),
        ] {
            let answer = mesh.call("embedding", "embed", request.clone()).await;
            assert!(
                answer["error"].is_object() || answer["result"]["isError"] == true,
                "{request}: {answer}"
            );
            let text = answer.to_string();
            assert!(
                !text.contains("synthetic-secret") && !text.contains(support::SIGNED_HEADER),
                "{text}"
            );
        }
        assert!(runtime_seen.lock().unwrap().is_empty());
        let today = chrono::Utc::now().date_naive();
        assert!(
            veoveo_usage::UsageStore::new(store)
                .daily(
                    &testing::authority().tenant,
                    today,
                    today.succ_opt().unwrap(),
                    20
                )
                .await
                .unwrap()
                .is_empty()
        );
        runtime_task.abort();
    })
    .await
    .expect("bounded refusal fixture exceeded 60s");
}

#[test]
fn the_composition_probe_reads_its_native_contract_declaration() {
    let caller = Forwarder::setup();
    use rmcp::model::{ReadResourceResult, ResourceContents};
    use support::forwarder::{PROBE_CONTRACT_URI, decode_probe_contract, probe_contract_uri};
    let content = |uri: &str, value: serde_json::Value| {
        ResourceContents::text(serde_json::to_string(&value).unwrap(), uri)
            .with_mime_type("application/json")
    };
    let good = serde_json::json!({"contract":"https://veoveo.ai/contracts/hosted-mcp","server":"embeddingprobe"});
    assert_eq!(probe_contract_uri().as_str(), PROBE_CONTRACT_URI);
    let parsed = decode_probe_contract(ReadResourceResult::new(vec![content(
        PROBE_CONTRACT_URI,
        good.clone(),
    )]))
    .unwrap();
    assert_eq!(parsed.server().as_str(), "embeddingprobe");
    for value in [
        serde_json::json!({"contract":"https://veoveo.ai/contracts/hosted-mcp","server":"embedding"}),
        serde_json::json!({"contract":"https://example.test/other","server":"embeddingprobe"}),
        serde_json::json!({"contract":"https://veoveo.ai/contracts/hosted-mcp","server":"embeddingprobe","unexpected":true}),
    ] {
        assert!(
            decode_probe_contract(ReadResourceResult::new(vec![content(
                PROBE_CONTRACT_URI,
                value
            )]))
            .is_err()
        );
    }
    assert!(
        decode_probe_contract(ReadResourceResult::new(vec![content(
            "embedding://contract",
            good.clone()
        )]))
        .is_err()
    );
    assert!(
        decode_probe_contract(ReadResourceResult::new(vec![
            content(PROBE_CONTRACT_URI, good.clone()),
            content(PROBE_CONTRACT_URI, good)
        ]))
        .is_err()
    );
    assert_eq!(caller.server_config().server_info.name, "embeddingprobe");
    assert_eq!(caller.documents().server(), "embeddingprobe");
}
