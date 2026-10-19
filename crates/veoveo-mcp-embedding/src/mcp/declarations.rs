//! The server's registration declaration. The server registers it with the Hub and
//! advertises the same value in MCP Discover, so both have one source.
use std::sync::OnceLock;

use veoveo_hub_sdk::{
    PublicDeployment,
    registration::{RegistrationDeclaration, ServerEndpoint, ServerVersion},
    server_contract::McpServerContract,
};

use super::setup::EmbeddingContract;

/// The registration the server advertises. Startup sets it from the deployment before
/// the MCP setup is built; until then Discover advertises no declarations.
pub static REGISTRATION: OnceLock<RegistrationDeclaration> = OnceLock::new();

/// The server's identity and the URL where the Hub reaches its MCP endpoint:
/// `{public_base_url}/embedding/mcp`.
pub fn registration(deployment: &PublicDeployment) -> anyhow::Result<RegistrationDeclaration> {
    let endpoint = deployment.server(crate::server().as_str())?;
    let url = url::Url::parse(&format!("{}/mcp", endpoint.public_url()))?;
    Ok(RegistrationDeclaration {
        slug: crate::server(),
        uri_scheme: EmbeddingContract::scheme(),
        version: ServerVersion::new(env!("CARGO_PKG_VERSION"))?,
        endpoint: ServerEndpoint::new(url)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_endpoint_is_the_mounted_mcp_url() {
        let deployment = PublicDeployment::new("http://vv-mcp-embedding.internal:8803").unwrap();
        let registration = registration(&deployment).unwrap();
        assert_eq!(
            registration.endpoint.url().as_str(),
            "http://vv-mcp-embedding.internal:8803/embedding/mcp"
        );
        assert_eq!(registration.slug.as_str(), "embedding");
        assert_eq!(registration.uri_scheme.as_str(), "embedding");
    }
}
