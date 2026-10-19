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
    #[arg(long, env = "VEOVEO_PORT", default_value_t = 8803)]
    pub port: u16,
    /// Fixed private runtime origin; credentials are mediated by native OpenShell.
    #[arg(long, env = "VEOVEO_EMBEDDING_RUNTIME_URL")]
    pub runtime_url: Url,
    #[arg(long, env = "VEOVEO_EMBEDDING_RUNTIME_TOKEN", hide_env_values = true, value_parser = parse_secret)]
    pub runtime_token: SecretString,
    #[arg(long, env = "VEOVEO_EMBEDDING_DEVICE", value_parser = ["mps", "cuda"])]
    pub device: String,
    /// The origin where the Hub reaches the server; it serves MCP at
    /// `{public_base_url}/embedding/mcp` and registers that URL.
    #[arg(long, env = "VEOVEO_PUBLIC_BASE_URL")]
    pub public_base_url: String,
    /// The Hub's RFC 8414 issuer; registration discovers its endpoint from there.
    #[arg(long, env = "VEOVEO_HUB_ISSUER")]
    pub hub_issuer: Url,
    /// The installation's registration secret, or the placeholder the installation's edge
    /// replaces.
    #[arg(
        long,
        env = "VEOVEO_REGISTRATION_SECRET",
        hide_env_values = true,
        value_parser = parse_secret
    )]
    pub registration_secret: SecretString,
    /// Holds the server's registration key, which keeps its namespace across restarts.
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
