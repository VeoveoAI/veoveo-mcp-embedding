//! The server's configuration. Every flag has its environment variable. The Hub
//! issues the database login and the access-token trust when the server registers, so
//! neither is configured here.
use std::path::PathBuf;

use clap::Parser;
use secrecy::SecretString;
use url::Url;
use veoveo_hub_sdk::{PublicDeployment, parse_allowed_host_authority};

#[derive(Parser)]
#[command(name = "veoveo-mcp-embedding", about = "Embedding MCP server")]
pub(super) struct Args {
    /// Print public registration metadata without network, database or runtime access.
    #[arg(long)]
    pub describe_registration: bool,
    /// Immutable worker identity, retained with its private key across restarts.
    #[arg(long, env = "VEOVEO_DEPLOYMENT_ID")]
    pub deployment_id: veoveo_hub_sdk::DeploymentId,
    #[arg(long, env = "VEOVEO_PORT", default_value_t = 8803)]
    pub port: u16,
    /// Fixed private runtime origin; credentials are mediated by native OpenShell.
    #[arg(
        long,
        env = "VEOVEO_EMBEDDING_RUNTIME_URL",
        required_unless_present = "describe_registration"
    )]
    pub runtime_url: Option<Url>,
    #[arg(long, env = "VEOVEO_EMBEDDING_RUNTIME_TOKEN", required_unless_present = "describe_registration", hide_env_values = true, value_parser = parse_secret)]
    pub runtime_token: Option<SecretString>,
    #[arg(long, env = "VEOVEO_EMBEDDING_DEVICE", required_unless_present = "describe_registration", value_parser = ["mps", "cuda"])]
    pub device: Option<String>,
    /// The origin where the Hub reaches the server; it serves MCP at
    /// `{public_base_url}/embedding/mcp` and registers that URL.
    #[arg(long, env = "VEOVEO_PUBLIC_BASE_URL")]
    pub public_base_url: String,
    /// The Hub's RFC 8414 issuer; registration discovers its endpoint from there.
    #[arg(
        long,
        env = "VEOVEO_HUB_ISSUER",
        required_unless_present = "describe_registration"
    )]
    pub hub_issuer: Option<Url>,
    /// Initial namespace or deployment-scoped admission, mediated by the installation.
    #[arg(
        long,
        env = "VEOVEO_REGISTRATION_SECRET",
        required_unless_present = "describe_registration",
        hide_env_values = true,
        value_parser = parse_secret
    )]
    pub registration_secret: Option<SecretString>,
    /// Private deployment key and native saved registration retained across restarts.
    #[arg(
        long,
        env = "VEOVEO_KEY_DIRECTORY",
        default_value = "/var/lib/veoveo/embedding/keys"
    )]
    pub key_directory: PathBuf,
    #[arg(long, env = "VEOVEO_ALLOW_LOOPBACK_HOSTS", default_value_t = false)]
    pub allow_loopback_hosts: bool,
    #[arg(
        long = "allowed-host",
        env = "VEOVEO_ALLOWED_HOSTS",
        value_name = "HOST",
        value_delimiter = ',',
        value_parser = parse_allowed_host
    )]
    pub allowed_hosts: Vec<String>,
}

impl Args {
    pub fn public_deployment(&self) -> anyhow::Result<PublicDeployment> {
        PublicDeployment::new(&self.public_base_url)
    }
}

fn parse_secret(value: &str) -> Result<SecretString, String> {
    (!value.is_empty())
        .then(|| SecretString::from(value))
        .ok_or_else(|| "secret must not be empty".to_owned())
}

fn parse_allowed_host(value: &str) -> Result<String, String> {
    let value = value.trim();
    parse_allowed_host_authority(value)
        .map(|_| value.to_owned())
        .ok_or_else(|| {
            "expected a host authority such as vv-mcp-embedding.internal:8803".to_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_description_does_not_require_runtime_or_admission_secrets() {
        let parsed = Args::try_parse_from([
            "veoveo-mcp-embedding",
            "--describe-registration",
            "--deployment-id",
            "019a5b30-0000-7000-8000-000000000001",
            "--public-base-url",
            "https://embedding.example",
        ])
        .unwrap();
        assert!(parsed.runtime_url.is_none());
        assert!(parsed.runtime_token.is_none());
        assert!(parsed.device.is_none());
        assert!(parsed.hub_issuer.is_none());
        assert!(parsed.registration_secret.is_none());
        assert!(
            Args::try_parse_from([
                "veoveo-mcp-embedding",
                "--describe-registration",
                "--public-base-url",
                "https://embedding.example",
            ])
            .is_err()
        );
        assert!(
            Args::try_parse_from([
                "veoveo-mcp-embedding",
                "--describe-registration",
                "--deployment-id",
                "invalid",
                "--public-base-url",
                "https://embedding.example",
            ])
            .is_err()
        );
        assert!(
            Args::try_parse_from([
                "veoveo-mcp-embedding",
                "--deployment-id",
                "019a5b30-0000-7000-8000-000000000001",
                "--public-base-url",
                "https://embedding.example",
            ])
            .is_err()
        );
    }
}
