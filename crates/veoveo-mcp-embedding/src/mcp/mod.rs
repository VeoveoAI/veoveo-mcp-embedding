//! Ready routes own discovery, authorization, typed dispatch and resource policy.
mod declarations;
mod requirements;
mod setup;

use std::{sync::LazyLock, time::Duration};

use rmcp::{ErrorData, RoleServer, service::RequestContext};
use veoveo_hub_sdk::{
    hosting::{DomainServer, ReadCache, ResourceBuilder, RoutePolicy, Routes, hub_identity},
    server_contract::McpServerSetup,
};
use veoveo_usage::UsageActor;

use crate::{
    contract::*,
    provider::RuntimeClient,
    usage::{CoreMeter, GenerationError, generate, usage_via},
};

pub use declarations::{REGISTRATION, registration};
pub use requirements::AUTHORIZATION;
pub use setup::EmbeddingContract;

pub fn force_setup() {
    LazyLock::force(&setup::SERVER_SETUP);
}

#[derive(Clone)]
pub struct EmbeddingMcp {
    runtime: RuntimeClient,
    meter: CoreMeter,
    capabilities: Capabilities,
}
impl EmbeddingMcp {
    pub fn new(runtime: RuntimeClient, meter: CoreMeter, capabilities: Capabilities) -> Self {
        Self {
            runtime,
            meter,
            capabilities,
        }
    }
    async fn embed(
        &self,
        request: EmbedRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<EmbeddingResult, ErrorData> {
        request
            .validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let identity = hub_identity(&context)?;
        let via = usage_via(identity.acting_server(), identity.acting_task())
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let actor = UsageActor {
            tenant: identity.authority.tenant,
            principal: identity.actor.id,
            work_context: identity.authority.work_context,
        };
        let request = RuntimeRequest {
            request_id: uuid::Uuid::now_v7(),
            items: request.items,
        };
        let id = request.request_id;
        let result = tokio::time::timeout(
            Duration::from_secs(REQUEST_SECONDS),
            generate(&self.runtime, &self.meter, &request, actor, via),
        )
        .await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(GenerationError::NotStarted(code))) if code.is_input_error() => {
                Err(ErrorData::invalid_params(
                    format!(
                        "embedding request {id}: {code}; inference not started; zero actual usage recorded"
                    ),
                    Some(serde_json::json!({"code":code,"execution":"not_started","requestId":id})),
                ))
            }
            Ok(Err(error)) => Err(ErrorData::internal_error(
                format!("embedding request {id}: {error}"),
                None,
            )),
            Err(_) => Err(ErrorData::internal_error(
                format!(
                    "embedding request {id} exceeded its deadline; execution/accounting may be incomplete; no automatic replay"
                ),
                None,
            )),
        }
    }
}

static ROUTES: LazyLock<Routes<EmbeddingMcp>> = LazyLock::new(|| {
    let read = RoutePolicy::permission(EmbeddingPermission::ModelsRead)
        .expect("embedding read permission");
    Routes::<EmbeddingMcp>::builder(read.clone())
        .tool::<EmbedRequest, _, _>(
            "Generate a bounded, ordered batch of normalized 768-dimensional text/code embeddings. Select each item's search_query, code_query or document purpose. Overflow is rejected, never truncated. No content is persisted; a retry starts new computation.",
            RoutePolicy::permission(EmbeddingPermission::EmbeddingsCreate).expect("embedding create permission"),
            |domain, request, context| async move { domain.embed(request, context).await.map_err(Into::into) },
        )
        .singleton::<ReadCapabilities, _, _>(ReadCapabilities,
            ResourceBuilder::new("Embedding capabilities (validated startup snapshot)")
                .reader(|domain: EmbeddingMcp, _request, _context| async move { Ok(domain.capabilities.clone()) })
                .policy(read)
                .cache(ReadCache::NoStore))
        .build()
});

impl DomainServer for EmbeddingMcp {
    type Contract = EmbeddingContract;
    fn setup() -> &'static McpServerSetup<EmbeddingContract> {
        &setup::SERVER_SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}
