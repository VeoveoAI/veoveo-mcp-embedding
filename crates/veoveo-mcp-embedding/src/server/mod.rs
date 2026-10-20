//! The hosted process: register with the Hub, connect with the registration's
//! database login, hold a process instance, migrate, then serve MCP until SIGTERM or
//! Ctrl-C or until the instance loses its lock. Stopping deletes the registration.
mod config;
mod registration;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use clap::Parser;
use veoveo_hub_sdk::{
    PublicDeployment, TelemetryGuard,
    hosting::{Hosted, HostedServer, ServerCalls},
    init_server_telemetry,
    registration::client::Registration,
};
use veoveo_store::{Instance, PlatformStore, StoreConfig};

use crate::{
    contract::{Device, MAX_JSON_BYTES},
    mcp::{self, EmbeddingMcp},
    provider::RuntimeClient,
    usage::CoreMeter,
};

use config::Args;

pub async fn run() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = dotenvy::dotenv();
    let _telemetry: TelemetryGuard =
        init_server_telemetry("veoveo-mcp-embedding", "info,veoveo_mcp_embedding=debug")?;
    let args = Args::parse();
    let deployment = args.public_deployment()?;
    let declaration = mcp::registration(&deployment)?;
    mcp::REGISTRATION
        .set(declaration.clone())
        .map_err(|_| anyhow::anyhow!("the registration was declared twice"))?;
    mcp::force_setup();
    let (client, registration, http, calls) = registration::register(&args, declaration).await?;
    // The registration lasts while the server's login holds a live process instance.
    // Once serving ends, on SIGTERM or Ctrl-C or on a failure, the client deletes it,
    // which also drops the login.
    let issued = registration.clone();
    let mut served = None;
    let maintained = client
        .maintain(registration, async {
            served = Some(serve(&args, &deployment, &issued, http, calls).await);
        })
        .await;
    if let Err(error) = maintained {
        tracing::warn!(%error, "the server could not remove its registration");
    }
    served.context("the registration ended before serving finished")?
}

/// Connect with the registration's database login, migrate the server's schema and
/// serve until SIGTERM or Ctrl-C, or until the process instance loses its lock.
async fn serve(
    args: &Args,
    deployment: &PublicDeployment,
    registration: &Registration,
    http: reqwest::Client,
    calls: ServerCalls,
) -> Result<()> {
    let grant = registration
        .database()
        .context("the Hub issued no database login")?;
    let store = PlatformStore::connect(
        StoreConfig::builder(grant.url.as_str(), grant.password.clone())
            .username(grant.username.as_str())
            .build()?,
    )
    .await
    .context("connecting to the installation database")?;
    let instance = Instance::start(&store, &crate::server())
        .await
        .context("starting the process instance")?;
    store
        .migrate_server(&crate::server(), crate::migrator())
        .await
        .context("applying the server's migrations")?;
    let meter = CoreMeter::new(store.clone())
        .await
        .context("declaring embedding usage")?;
    let runtime = RuntimeClient::new(
        args.runtime_url.clone(),
        args.runtime_token.clone(),
        if args.device == "mps" {
            Device::Mps
        } else {
            Device::Cuda
        },
    )?;
    let capabilities =
        tokio::time::timeout(std::time::Duration::from_secs(3), runtime.capabilities())
            .await
            .context("runtime startup readiness timed out")?
            .context("runtime does not admit the fixed model contract")?;
    let domain = EmbeddingMcp::new(runtime.clone(), meter, capabilities);
    let readiness = runtime.clone();
    let hosted = HostedServer::for_domain::<EmbeddingMcp>()
        .deployment(deployment, args.allow_loopback_hosts)?
        .allowed_hosts(args.allowed_hosts.iter().cloned())
        .internal_trust(registration.access_tokens(), http)?
        .calls(calls)
        .handler(move || Hosted::new(domain.clone()))
        .mcp_request_limit(MAX_JSON_BYTES)
        .request_timeout(std::time::Duration::from_secs(65))
        .readiness(move || {
            let runtime = readiness.clone();
            async move { runtime.ready().await }
        })
        .build();
    let served = tokio::select! {
        served = hosted.serve(SocketAddr::from(([0, 0, 0, 0], args.port))) => served,
        () = instance.lost() => Err(anyhow::anyhow!("the process instance lost its lock")),
    };
    if let Err(error) = instance.shutdown().await {
        tracing::warn!(%error, "the process instance did not stop cleanly");
    }
    store.close().await;
    served
}
