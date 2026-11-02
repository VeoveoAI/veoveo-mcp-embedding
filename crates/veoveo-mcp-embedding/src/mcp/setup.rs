//! The server's checked contract: slug, scheme, scope, documents, skills, the fixed
//! resources it lists and the embeddings it declares.
use std::sync::LazyLock;

use rmcp::model::{ServerCapabilities, ServerConfig};
use veoveo_hub_sdk::{
    docs::ServerDocs,
    server_contract::{McpServerContract, McpServerSetup},
    skills::ServerSkills,
};
use veoveo_types::ResourceScheme;

use crate::contract::{EmbeddingProvider, EmbeddingScope};

/// The crate documents embedded at build time and served under `embedding://docs`,
/// `embedding://docs/{doc_id}`, `embedding://contract` and the admin document routes.
pub(crate) static SERVER_DOCS: LazyLock<ServerDocs> =
    LazyLock::new(|| veoveo_hub_sdk::server_docs!("embedding"));

/// The skills under `skills/`, embedded at build time and served at `skill://embedding/…`.
pub(crate) static SERVER_SKILLS: LazyLock<ServerSkills> =
    LazyLock::new(|| veoveo_hub_sdk::server_skills!("embedding"));

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
    fn skills() -> &'static ServerSkills {
        &SERVER_SKILLS
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
    info.instructions = Some(INSTRUCTIONS.to_owned());
    info
}

/// What the server is and which skill says how to use it.
const INSTRUCTIONS: &str = "Embedding turns ordered text, image, audio and video content \
     into normalized vectors with one fixed multimodal model, EmbeddingGemma 2. Its skill \
     says how: skill://embedding/embed-content/SKILL.md.";

#[cfg(test)]
mod tests {
    #[test]
    fn the_instructions_name_every_skill_the_server_serves() {
        let skills = &*super::SERVER_SKILLS;
        assert_eq!(skills.skills().len(), 1);
        for skill in skills.skills() {
            assert!(super::INSTRUCTIONS.contains(&skill.uri), "{}", skill.uri);
        }
    }
}
