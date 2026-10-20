//! Opt-in temporary native caller. Operator supplies the existing registration
//! credential selector and an admitted endpoint; this code installs no policy.
use crate::{
    Installation, bearer, required,
    support::forwarder::{Forwarder, ForwarderContract, decode_probe_contract, probe_contract_uri},
};
use anyhow::{Result, anyhow, ensure};
use std::{
    future::Future,
    net::SocketAddr,
    time::{Duration, Instant},
};
use veoveo_hub_sdk::{
    PublicDeployment,
    hosting::{DomainServer, Hosted, HostedServer, ServerCalls},
    registration::{
        ClientMetadata, RegistrationDeclaration, ServerEndpoint, ServerVersion,
        client::RegistrationClient, statement::ServerKeyPair,
    },
};
use veoveo_hub_sdk::{calls::ProviderContract, server_contract::McpServerContract};
use veoveo_store::{Instance, PlatformStore, StoreConfig};
const PHASE: Duration = Duration::from_secs(10);
async fn phase<T>(
    future: impl Future<Output = Result<T, impl std::fmt::Debug>>,
    label: &'static str,
) -> Result<T> {
    tokio::time::timeout(PHASE, future)
        .await
        .map_err(|_| anyhow!(label))?
        .map_err(|_| anyhow!(label))
}
struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReadiness {
    tools_list_calls: usize,
    contract_reads: usize,
    elapsed_ms: f64,
    contract: veoveo_hub_sdk::docs::ContractDeclaration,
    tool: &'static str,
}
pub struct ProbeConfig {
    deployment_id: veoveo_hub_sdk::DeploymentId,
    deployment: PublicDeployment,
    bind: SocketAddr,
    secret: secrecy::SecretString,
    allow_loopback: bool,
    allowed_hosts: Vec<String>,
}
impl ProbeConfig {
    pub fn load() -> Result<Self> {
        ensure!(
            std::env::var("VEOVEO_ACCEPTANCE_COMPOSED_PROBE").as_deref() == Ok("1"),
            "explicit COMPOSED_PROBE=1 required"
        );
        let deployment_id = veoveo_hub_sdk::DeploymentId::parse(required("PROBE_DEPLOYMENT_ID")?)
            .map_err(|_| anyhow!("invalid admitted probe deployment UUIDv7"))?;
        let deployment = PublicDeployment::new(&required("PROBE_PUBLIC_BASE_URL")?)
            .map_err(|_| anyhow!("invalid admitted probe origin"))?;
        let bind: SocketAddr = required("PROBE_BIND")?
            .parse()
            .map_err(|_| anyhow!("PROBE_BIND must be an explicit socket address"))?;
        ensure!(
            bind.port() != 0,
            "probe port must be explicitly admitted, not ephemeral"
        );
        let secret = secrecy::SecretString::from(bearer(&required("REGISTRATION_SECRET_ENV")?)?);
        let allowed_hosts = required("PROBE_ALLOWED_HOSTS")?
            .split(',')
            .map(str::trim)
            .map(|host| {
                veoveo_hub_sdk::parse_allowed_host_authority(host)
                    .map(|_| host.to_owned())
                    .ok_or_else(|| anyhow!("invalid admitted probe host authority"))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !allowed_hosts.is_empty(),
            "admitted probe host authorities required"
        );
        Ok(Self {
            deployment_id,
            deployment,
            bind,
            secret,
            allow_loopback: std::env::var("VEOVEO_ACCEPTANCE_PROBE_ALLOW_LOOPBACK").as_deref()
                == Ok("1"),
            allowed_hosts,
        })
    }
    pub async fn run<F, Fut>(&self, installation: &Installation, runner: F) -> Result<()>
    where
        F: FnOnce(ProbeReadiness) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let slug = ForwarderContract::slug();
        let endpoint = self
            .deployment
            .server(slug.as_str())
            .map_err(|_| anyhow!("invalid probe mount"))?;
        let declaration = RegistrationDeclaration {
            deployment: self.deployment_id,
            slug: slug.clone(),
            uri_scheme: ForwarderContract::scheme(),
            version: ServerVersion::new(env!("CARGO_PKG_VERSION"))
                .map_err(|_| anyhow!("invalid probe version"))?,
            endpoint: ServerEndpoint::new(
                url::Url::parse(&format!("{}/mcp", endpoint.public_url()))
                    .map_err(|_| anyhow!("invalid probe endpoint"))?,
            )
            .map_err(|_| anyhow!("invalid probe endpoint"))?,
        };
        let key =
            ServerKeyPair::generate().map_err(|_| anyhow!("native probe key generation failed"))?;
        let call_key = ServerKeyPair::from_pkcs8_der(key.pkcs8_der())
            .map_err(|_| anyhow!("native probe key copy failed"))?;
        let metadata = ClientMetadata::new(
            *key.public_key(),
            declaration,
            Forwarder::routes().authorization(),
        )
        .map_err(|_| anyhow!("native probe declaration failed"))?;
        let endpoint = phase(
            RegistrationClient::discover(&installation.http, &installation.origin),
            "probe registration discovery failed",
        )
        .await?;
        let client = RegistrationClient::new(
            installation.http.clone(),
            endpoint,
            self.secret.clone(),
            key,
            metadata.clone(),
        )
        .map_err(|_| anyhow!("native probe registration setup failed"))?;
        let registration = phase(
            client.register(),
            "probe registration failed; no alternate credential/path attempted",
        )
        .await?;
        let mut outcome = None;
        // Process stop preserves credentials; this disposable fixture explicitly retires afterward.
        let maintained = client
            .maintain(registration.clone(), async {
                outcome = Some(async {
                    let grant = registration.database().ok_or_else(|| {
                        anyhow!("probe registration supplied no native database login")
                    })?;
                    let config = StoreConfig::builder(grant.url.as_str(), grant.password.clone())
                        .username(grant.username.as_str())
                        .max_connections(2)
                        .build()
                        .map_err(|_| anyhow!("probe store configuration failed"))?;
                    let store = phase(PlatformStore::connect(config), "probe store connection failed").await?;
                    let instance = phase(Instance::start(&store, &slug), "native probe process instance failed").await?;
                    let work = async {
                        let calls = phase(
                            ServerCalls::discover(&registration, &metadata, &call_key, installation.http.clone()),
                            "native probe token discovery failed",
                        ).await?;
                        let hosted = HostedServer::for_domain::<Forwarder>()
                            .deployment(&self.deployment, self.allow_loopback)
                            .map_err(|_| anyhow!("probe deployment rejected"))?
                            .allowed_hosts(self.allowed_hosts.iter().cloned())
                            .internal_trust(registration.access_tokens(), installation.http.clone())
                            .map_err(|_| anyhow!("probe native token trust failed"))?
                            .calls(calls)
                            .handler(|| Hosted::new(Forwarder))
                            .mcp_request_limit(crate::MAX_JSON_BYTES)
                            .request_timeout(crate::DEADLINE)
                            .build();
                        let (stop, stopped) = tokio::sync::oneshot::channel();
                        let mut serving = tokio::spawn(hosted.serve_with_shutdown(self.bind, async move {
                            let _ = stopped.await;
                        }));
                        let _abort = AbortOnDrop(serving.abort_handle());
                        // The Hub is the actual caller. Its native routing must
                        // discover this exact fixture, not a separate ingress URL.
                        let discovery = installation.client(&installation.member)?;
                        let ready = async {
                            let started = Instant::now();
                            let (mut tools_list_calls, mut contract_reads) = (0, 0);
                            for _ in 0..40 {
                                tools_list_calls += 1;
                                if let Ok(tools) = discovery.tools().await
                                    && tools.items.iter().any(|tool| tool.name == "embeddingprobe__forward_embed") {
                                    contract_reads += 1;
                                    if let Ok(result) = discovery.read_resource(&probe_contract_uri()).await {
                                        let contract = decode_probe_contract(result).map_err(|_| anyhow!("native probe contract response differs"))?;
                                        return Ok(ProbeReadiness {
                                            tools_list_calls, contract_reads,
                                            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                                            contract, tool: "embeddingprobe__forward_embed",
                                        });
                                    }
                                }
                                // Only metadata reads poll; no inference is retried.
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                            Err(anyhow!("native Hub did not discover the exact probe contract"))
                        };
                        let mut listener_ended = false;
                        let result = async {
                            let readiness = phase(ready, "native probe discovery readiness deadline").await?;
                            tokio::select! {
                                result = tokio::time::timeout(Duration::from_secs(120), runner(readiness)) => {
                                    result.map_err(|_| anyhow!("probe work deadline; execution/accounting may be incomplete; no replay"))?
                                }
                                () = instance.lost() => Err(anyhow!("probe process instance lost")),
                                result = &mut serving => {
                                    listener_ended = true;
                                    let _ = result;
                                    Err(anyhow!("probe listener ended during qualification"))
                                }
                            }
                        }.await;
                        let _ = stop.send(());
                        let shutdown = if listener_ended { Ok(()) } else {
                            match tokio::time::timeout(PHASE, &mut serving).await {
                                Ok(Ok(Ok(()))) => Ok(()),
                                _ => Err(anyhow!("probe listener shutdown failed or exceeded deadline")),
                            }
                        };
                        result.and(shutdown)
                    }.await;
                    let stopped = phase(instance.shutdown(), "probe process shutdown failed").await;
                    let closed = tokio::time::timeout(PHASE, store.close()).await
                        .map_err(|_| anyhow!("probe store close exceeded deadline"));
                    work.and(stopped).and(closed)
                }.await);
            })
            .await;
        ensure!(
            maintained.is_ok(),
            "native probe lifecycle failed; operator must inspect registration state"
        );
        phase(
            client.deregister(registration),
            "native probe retirement failed",
        )
        .await?;
        outcome.ok_or_else(|| anyhow!("probe lifecycle did not run"))?
    }
}
