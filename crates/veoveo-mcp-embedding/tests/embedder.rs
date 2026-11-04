//! `EmbeddingClient`, the SDK's knowledge `Embedder`, called as a knowledge source calls it:
//! from a probe server's handler through its caller-bound `HubClient`, through the SDK's
//! test Hub, to Embedding over a synthetic runtime. Vectors and signed URLs are synthetic.
#![cfg(feature = "server")]
mod support;
use rmcp::{
    RoleServer,
    model::{ServerCapabilities, ServerConfig},
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use veoveo_hub_sdk::{
    calls::{ProviderContract, ToolCall},
    client::{HubClient, StaticBearer},
    docs::ServerDocs,
    hosting::{
        ClientContext, DomainServer, Hosted, RoutePolicy, Routes, ToolError,
        testing::{self, TestMesh},
    },
    knowledge::{ContentParts, Embedder, EmbeddingPurpose, Embeddings},
    registration::{Access, PermissionDefinition},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_mcp_embedding::{
    client::{EmbedError, EmbeddingClient},
    contract::*,
    mcp::EmbeddingMcp,
    provider::RuntimeClient,
    usage::CoreMeter,
};
use veoveo_types::{ResourceScheme, ServerSlug};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum NoScope {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
enum ProbePermission {
    #[vocabulary(rename = "knowledgeprobe.records.index")]
    Index,
}
impl PermissionDefinition for ProbePermission {
    fn resource(self) -> &'static str {
        "knowledgeprobe.record"
    }
    fn access(self) -> Access {
        Access::Read
    }
}

/// A synthetic knowledge source that embeds items for its caller.
#[derive(Clone)]
struct Probe;
struct ProbeContract;
impl ProviderContract for ProbeContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("knowledgeprobe").unwrap()
    }
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmbedItems {
    items: Vec<ContentParts>,
    purpose: EmbeddingPurpose,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, veoveo_hub_sdk::SingleOutput)]
#[serde(deny_unknown_fields)]
struct Embedded {
    embeddings: Embeddings,
}
impl ToolCall for EmbedItems {
    type Provider = ProbeContract;
    type Output = Embedded;
    const TOOL: &'static str = "embed_items";
}
static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(ProbeContract::slug())
        .with_doc(
            "agents",
            "Synthetic fixture",
            "# Synthetic knowledge source",
        )
        .with_doc("design", "Design", "# Synthetic tests only")
});
static SETUP: LazyLock<McpServerSetup<ProbeContract>> =
    LazyLock::new(|| McpServerSetup::new().unwrap());
static ROUTES: LazyLock<Routes<Probe>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .uses(EmbeddingPermission::EmbeddingsCreate.name())
        .tool::<EmbedItems, _, _>(
            "Embed items for the caller",
            RoutePolicy::permission(ProbePermission::Index).unwrap(),
            |_, request: EmbedItems, context: RequestContext<RoleServer>| async move {
                let client = context.client()?;
                let embeddings = EmbeddingClient
                    .embed(&client, &request.items, request.purpose)
                    .await
                    .map_err(|error| ToolError::Rejected(error.to_string()))?;
                Ok(Embedded { embeddings })
            },
        )
        .build()
});
impl McpServerContract for ProbeContract {
    type Server = Probe;
    type Scope = NoScope;
    type Provider = ProbeContract;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("knowledgeprobe").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        &[]
    }
    fn documents() -> &'static ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        config.server_info =
            rmcp::model::Implementation::new("knowledgeprobe", env!("CARGO_PKG_VERSION"));
        config
    }
}
impl DomainServer for Probe {
    type Contract = ProbeContract;
    fn setup() -> &'static McpServerSetup<ProbeContract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}

#[tokio::test]
async fn a_knowledge_source_embeds_documents_and_queries_through_its_callers_client() {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = veoveo_fixtures::store::TestDb::new().await;
        let store = db.server_store(&veoveo_mcp_embedding::server()).await;
        let (origin, runtime_seen, runtime_task) = support::runtime().await;
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
        let calls = mesh.register(Probe::routes().authorization());
        mesh.serve(
            testing::for_domain::<Probe>()
                .calls(calls)
                .handler(|| Hosted::new(Probe))
                .build(),
        );
        let mesh = mesh
            .granting([
                EmbeddingPermission::EmbeddingsCreate.name(),
                veoveo_types::PermissionName::parse("knowledgeprobe.records.index").unwrap(),
            ])
            .start();
        let items = serde_json::json!([
            [{"type": "text", "text": "a red kite over the dunes"}],
            [{"type": "text", "text": "the same scene"}, support::media_json("image", "image/png", 3)]
        ]);
        for (purpose, prompt) in [("document", PromptName::Document), ("query", PromptName::SearchQuery)] {
            let answer = mesh
                .call(
                    "knowledgeprobe",
                    "embed_items",
                    serde_json::json!({"items": items, "purpose": purpose}),
                )
                .await;
            assert_ne!(answer["result"]["isError"], true, "{answer}");
            assert!(!answer.to_string().contains("synthetic-secret"));
            let embedded: Embedded =
                serde_json::from_value(answer["result"]["structuredContent"].clone()).unwrap();
            let embeddings = embedded.embeddings;
            assert_eq!(
                embeddings.model().checkpoint.as_str(),
                format!("{MODEL_ID}@{MODEL_REVISION}")
            );
            assert_eq!(embeddings.model().dimensions.get(), 768);
            assert_eq!(embeddings.vectors().len(), 2);
            assert!(embeddings.vectors().iter().all(|vector| vector.len() == 768));
            let sent = runtime_seen.lock().unwrap();
            let request = sent.last().unwrap();
            assert_eq!(request.prompt_name, Some(prompt));
            assert_eq!(request.dimensions, Dimensions::D768);
            assert_eq!(request.preprocessing, Preprocessing::default().effective());
            assert_eq!(
                request.items[1].content[1].source().unwrap().get.url.expose(),
                support::SIGNED_URL
            );
        }
        runtime_task.abort();
    })
    .await
    .expect("bounded embedder fixture exceeded 60s");
}

#[tokio::test]
async fn items_beyond_one_request_are_refused_before_any_call() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // The endpoint is never reached: the client refuses before it sends anything.
    let client = HubClient::new(
        url::Url::parse("http://127.0.0.1:9/mcp/local").unwrap(),
        reqwest::Client::new(),
        StaticBearer::new("synthetic-bearer".to_owned()),
    );
    let item = ContentParts::one(ContentPart::text("a")).unwrap();
    let error = EmbeddingClient
        .embed(
            &client,
            &vec![item.clone(); MAX_ITEMS + 1],
            EmbeddingPurpose::Document,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, EmbedError::Request(_)), "{error}");
    let error = EmbeddingClient
        .embed(&client, &[], EmbeddingPurpose::Query)
        .await
        .unwrap_err();
    assert!(matches!(error, EmbedError::Request(_)), "{error}");
}

#[test]
fn a_result_that_does_not_answer_its_items_in_order_is_refused() {
    let item = ContentParts::one(ContentPart::text("a")).unwrap();
    let request = EmbeddingClient::request(&[item.clone(), item], EmbeddingPurpose::Document);
    assert_eq!(request.prompt_name, Some(PromptName::Document));
    let runtime =
        veoveo_mcp_embedding::protocol::RuntimeRequest::new(uuid::Uuid::now_v7(), &request);
    let result = support::output(&runtime);
    assert!(EmbeddingClient::embeddings(result.clone(), 2).is_ok());
    assert!(matches!(
        EmbeddingClient::embeddings(result.clone(), 3),
        Err(EmbedError::Result)
    ));
    let mut swapped = result;
    swapped.items.swap(0, 1);
    assert!(matches!(
        EmbeddingClient::embeddings(swapped, 2),
        Err(EmbedError::Result)
    ));
}
