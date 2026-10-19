//! Synthetic boundary: native SDK discovery/permissions; no model inference.
#![cfg(feature = "server")]
use veoveo_hub_sdk::hosting::{
    Hosted,
    testing::{self, TestHub},
};
use veoveo_mcp_embedding::{
    contract::*, mcp::EmbeddingMcp, provider::RuntimeClient, usage::CoreMeter,
};
#[tokio::test]
async fn native_discovery_does_not_grant_embedding_compute_from_read_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = veoveo_fixtures::store::TestDb::new().await;
    let store = db.server_store(&veoveo_mcp_embedding::server()).await;
    let runtime = RuntimeClient::new(
        url::Url::parse("http://127.0.0.1:9").unwrap(),
        secrecy::SecretString::from("synthetic-placeholder"),
        Device::Mps,
    )
    .unwrap();
    let capabilities: Capabilities = serde_json::from_value(serde_json::json!({
        "model":{"id":MODEL_ID,"revision":MODEL_REVISION},"processingId":PROCESSING_ID,
        "dimensions":768,"normalized":true,"execution":{"device":"mps","dtype":"float32"},
        "runtime":{"package":"veoveo-runtime-embedding","version":"0.1.0","sourceRevision":"synthetic","python":"3.14.6","sentenceTransformers":"6.1.0","transformers":"5.20.2","torch":"2.14.1","torchvision":"0.29.1","pillow":"13.0.0"},
        "purposes":["search_query","code_query","document"],
        "limits":{"maxItems":8,"maxContentBytes":65536,"maxItemTokens":8192,"maxBatchTokens":16384}
    })).unwrap();
    let domain = EmbeddingMcp::new(runtime, CoreMeter::new(store).await.unwrap(), capabilities);
    let compute_domain = domain.clone();
    let hub = TestHub::new(
        testing::for_domain::<EmbeddingMcp>()
            .handler(move || Hosted::new(domain.clone()))
            .build(),
    )
    .granting([EmbeddingPermission::ModelsRead.name()]);
    let tools = hub.rpc("tools/list", serde_json::json!({})).await;
    assert_eq!(tools["result"]["tools"], serde_json::json!([]));
    let denied = hub.rpc("tools/call", serde_json::json!({"name":"embed","arguments":{"items":[{"purpose":"search_query","text":"synthetic"}]}})).await;
    assert!(denied["error"].is_object() || denied["result"]["isError"] == true);
    assert!(denied.to_string().contains("embedding.embeddings.create"));
    let caps = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri":"embedding://capabilities"}),
        )
        .await;
    assert!(caps["result"]["contents"].is_array());
    let compute = TestHub::new(
        testing::for_domain::<EmbeddingMcp>()
            .handler(move || Hosted::new(compute_domain.clone()))
            .build(),
    )
    .granting([
        EmbeddingPermission::ModelsRead.name(),
        EmbeddingPermission::EmbeddingsCreate.name(),
    ]);
    let tools = compute.rpc("tools/list", serde_json::json!({})).await;
    assert_eq!(tools["result"]["tools"][0]["name"], "embed");
}
