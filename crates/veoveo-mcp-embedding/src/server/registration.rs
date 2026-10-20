//! Registration with the Hub: the server's key, its declarations, and the client that
//! resumes the native saved registration across process restarts.
use anyhow::{Context, Result};
use veoveo_hub_sdk::hosting::ServerCalls;
use veoveo_hub_sdk::registration::{
    ClientMetadata, RegistrationDeclaration,
    client::{KeyDirectory, Registration, RegistrationClient},
    statement::ServerKeyPair,
};

use super::config::Args;
use crate::mcp::AUTHORIZATION;

/// Resume or initially register the server. Returns the lifecycle client, the
/// registration, and the HTTP client that reached the Hub, which also fetches the
/// Hub's token keys when a forwarded token names a key the registration lacks.
pub(super) async fn register(
    args: &Args,
    declaration: RegistrationDeclaration,
) -> Result<(
    RegistrationClient,
    Registration,
    reqwest::Client,
    ServerCalls,
)> {
    let keys = KeyDirectory::new(&args.key_directory);
    let metadata = prepare(args, declaration)?;
    let key = keys
        .load_or_generate()
        .context("loading the registration key")?;
    let call_key = ServerKeyPair::from_pkcs8_der(key.pkcs8_der())
        .context("retaining the same native registration key for server calls")?;
    let http = reqwest::Client::builder()
        .build()
        .context("building the registration HTTP client")?;
    let endpoint = RegistrationClient::discover(
        &http,
        args.hub_issuer
            .as_ref()
            .context("serving requires a Hub issuer")?,
    )
    .await
    .context("discovering the Hub's registration endpoint")?;
    let client = RegistrationClient::new(
        http.clone(),
        endpoint,
        args.registration_secret
            .clone()
            .context("serving requires initial registration admission")?,
        key,
        metadata.clone(),
    )?;
    let registration = client
        .resume_or_register(&keys)
        .await
        .context("registering with the Hub")?;
    tracing::info!(client_id = %registration.client_id(), "registered with the Hub");
    let calls = ServerCalls::discover(&registration, &metadata, &call_key, http.clone())
        .await
        .context("discovering the Hub's token endpoint")?;
    Ok((client, registration, http, calls))
}

/// One typed metadata path shared by offline admission and actual serving.
pub(super) fn prepare(args: &Args, declaration: RegistrationDeclaration) -> Result<ClientMetadata> {
    KeyDirectory::new(&args.key_directory)
        .prepare(declaration, AUTHORIZATION.clone())
        .context("preparing Embedding registration metadata")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn offline_prepare_reuses_worker_key_and_creates_no_registration_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let args = Args::try_parse_from([
            "veoveo-mcp-embedding",
            "--describe-registration",
            "--deployment-id",
            "019a5b30-0000-7000-8000-000000000001",
            "--public-base-url",
            "https://embedding.example",
            "--key-directory",
            directory.path().to_str().unwrap(),
        ])
        .unwrap();
        let declaration =
            crate::mcp::registration(&args.public_deployment().unwrap(), args.deployment_id)
                .unwrap();
        let first = prepare(&args, declaration.clone()).unwrap();
        assert_eq!(prepare(&args, declaration).unwrap(), first);
        assert_eq!(first.registration.deployment, args.deployment_id);
        assert!(
            !KeyDirectory::new(directory.path())
                .registration_path()
                .exists()
        );
        let metadata = serde_json::to_value(first).unwrap();
        assert!(metadata.get("veoveo_database").is_none());
        assert!(metadata.get("registration_access_token").is_none());
    }
}
