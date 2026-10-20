//! Reusable native forwarder for synthetic composition and opt-in deployed tests.
use rmcp::{
    RoleServer,
    model::{ServerCapabilities, ServerConfig},
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use veoveo_hub_sdk::{
    calls::ProviderContract,
    docs::ServerDocs,
    hosting::{ClientContext, DomainServer, RoutePolicy, Routes},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_mcp_embedding::contract::*;
use veoveo_types::{ResourceScheme, ServerSlug};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
pub enum NoScope {}
fn docs(slug: &str) -> ServerDocs {
    ServerDocs::new(ServerSlug::parse(slug).unwrap())
        .with_doc(
            "agents",
            "Probe manual",
            "# Explicit opt-in embedding composition probe",
        )
        .with_doc(
            "design",
            "Design",
            "# Temporary fixture only; no Tasks, storage or alternate credentials",
        )
}
fn config() -> ServerConfig {
    let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
    config.server_info =
        rmcp::model::Implementation::new("embeddingprobe", env!("CARGO_PKG_VERSION"));
    config
}
#[derive(Clone)]
pub struct Forwarder;
pub struct ForwarderContract;
impl ProviderContract for ForwarderContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("embeddingprobe").unwrap()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
enum ForwarderPermission {
    #[vocabulary(rename = "embeddingprobe.calls.make")]
    Make,
}
impl veoveo_hub_sdk::registration::PermissionDefinition for ForwarderPermission {
    fn resource(self) -> &'static str {
        "embeddingprobe.call"
    }
    fn access(self) -> veoveo_hub_sdk::registration::Access {
        veoveo_hub_sdk::registration::Access::Read
    }
    fn risks(self) -> &'static [veoveo_hub_sdk::registration::Risk] {
        &[]
    }
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Forward {
    pub request: EmbedRequest,
}
impl veoveo_hub_sdk::calls::ToolCall for Forward {
    type Provider = ForwarderContract;
    type Output = EmbeddingResult;
    const TOOL: &'static str = "forward_embed";
}
static CALLER_DOCS: LazyLock<ServerDocs> = LazyLock::new(|| docs("embeddingprobe"));
static CALLER_SETUP: LazyLock<McpServerSetup<ForwarderContract>> =
    LazyLock::new(|| McpServerSetup::new().unwrap());
static CALLER_ROUTES: LazyLock<Routes<Forwarder>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .uses(EmbeddingPermission::EmbeddingsCreate.name())
        .tool::<Forward, _, _>(
            "Synthetic composition",
            RoutePolicy::permission(ForwarderPermission::Make).unwrap(),
            |_, request, context: RequestContext<RoleServer>| async move {
                context
                    .client()?
                    .call(request.request)
                    .await
                    .map_err(Into::into)
            },
        )
        .build()
});
impl McpServerContract for ForwarderContract {
    type Server = Forwarder;
    type Scope = NoScope;
    type Provider = ForwarderContract;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("embeddingprobe").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        &[]
    }
    fn documents() -> &'static ServerDocs {
        &CALLER_DOCS
    }
    fn server_config() -> ServerConfig {
        config()
    }
}
impl DomainServer for Forwarder {
    type Contract = ForwarderContract;
    fn setup() -> &'static McpServerSetup<ForwarderContract> {
        &CALLER_SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &CALLER_ROUTES
    }
}
