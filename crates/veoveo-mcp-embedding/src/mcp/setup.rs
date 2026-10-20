//! The server's checked contract: slug, scheme, scope, documents, the fixed resources it
//! lists and the embeddings it declares.
use std::sync::LazyLock;

use rmcp::model::{ServerCapabilities, ServerConfig};
use veoveo_hub_sdk::{
    docs::ServerDocs,
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::ResourceScheme;

use crate::contract::{EmbeddingProvider, EmbeddingScope};

/// The crate documents embedded at build time and served under `embedding://docs`,
/// `embedding://docs/{doc_id}`, `embedding://contract` and the admin document routes.
pub(crate) static SERVER_DOCS: LazyLock<ServerDocs> =
    LazyLock::new(|| veoveo_hub_sdk::server_docs!("embedding"));

pub struct EmbeddingContract;

pub(crate) static SERVER_SETUP: LazyLock<McpServerSetup<EmbeddingContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("the embedding's MCP contract setup"));

impl McpServerContract for EmbeddingContract {
    type Scope = EmbeddingScope;
    type Server = super::EmbeddingMcp;
    type Provider = EmbeddingProvider;

    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("embedding").expect("declared embedding scheme")
    }
    fn scopes() -> &'static [EmbeddingScope] {
        <EmbeddingScope as veoveo_types::Vocabulary>::ALL
    }
    fn documents() -> &'static ServerDocs {
        &SERVER_DOCS
    }
    fn server_config() -> ServerConfig {
        server_config()
    }
}

fn server_config() -> ServerConfig {
    let mut capabilities = ServerCapabilities::builder()
        .enable_tools()
        .enable_resources()
        .enable_completions()
        .build();
    if let Some(registration) = super::declarations::REGISTRATION.get() {
        veoveo_hub_sdk::registration::mcp::advertise(
            &mut capabilities,
            registration,
            &super::requirements::AUTHORIZATION,
        );
    }
    let mut info = ServerConfig::default();
    info.capabilities = capabilities;
    info.server_info = rmcp::model::Implementation::new("embedding", env!("CARGO_PKG_VERSION"));
    info.instructions = Some(
        "Generate one normalized vector per ordered content item with the fixed full EmbeddingGemma 2 model. Combine ordered text/image/audio/video parts for a joint embedding; reference media by Artifact ID. Read embedding://capabilities for native controls, defaults and Veoveo service bounds. Dimensions default to768, with128/256/512 supported; optional request-wide promptName or prompt. Audio is not truncated unless explicitly requested. Content and vectors are transient; retries are new computation.".to_owned(),
    );
    info
}
