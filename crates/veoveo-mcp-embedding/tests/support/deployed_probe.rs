//! Opt-in temporary native caller. Operator supplies the existing registration
//! credential selector and an admitted endpoint; this code installs no policy.
use crate::{
    Installation, bearer, required,
    support::forwarder::{Forwarder, ForwarderContract},
};
use anyhow::{Result, anyhow, ensure};
use std::{future::Future, net::SocketAddr, time::Duration};
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
pub struct ProbeConfig {
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
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let slug = ForwarderContract::slug();
        let endpoint = self
            .deployment
            .server(slug.as_str())
            .map_err(|_| anyhow!("invalid probe mount"))?;
        let declaration = RegistrationDeclaration {
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
        // Native maintain always deregisters after the owned fixture future ends.
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
                        // Reachability uses the operator's admitted public origin;
                        // it never chooses an alternate loopback/proxy route.
                        let mounted = self.deployment.server(slug.as_str())
                            .map_err(|_| anyhow!("invalid probe mount"))?;
                        let mut origin = url::Url::parse(mounted.public_url())
                            .map_err(|_| anyhow!("invalid probe readiness origin"))?;
                        origin.set_path("/livez");
                        let ready = async {
                            for _ in 0..40 {
                                if installation.http.get(origin.clone())
                                    .timeout(Duration::from_secs(1)).send().await
                                    .is_ok_and(|response| response.status().is_success()) {
                                    return Ok(());
                                }
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                            Err(anyhow!("probe admitted endpoint did not become live"))
                        };
                        let mut listener_ended = false;
                        let result = async {
                            phase(ready, "probe readiness deadline").await?;
                            tokio::select! {
                                result = tokio::time::timeout(Duration::from_secs(120), runner()) => {
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
            "native probe deregistration failed; operator must inspect registration state"
        );
        outcome.ok_or_else(|| anyhow!("probe lifecycle did not run"))?
    }
}
