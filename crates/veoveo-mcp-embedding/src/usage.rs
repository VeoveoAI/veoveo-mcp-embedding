//! Nonmonetary usage through native Core APIs. No server-owned SQL or content storage.
use std::future::Future;

use chrono::{DateTime, Utc};
use veoveo_hub_sdk::OpaqueTaskId;
use veoveo_hub_sdk::usage::UsageKinds;
use veoveo_hub_sdk::usage::{
    ChargePeriod, Quantity, ServiceCategory, UsageKind, UsageKindDeclaration, UsagePhase,
    UsageSourceId, UsageUnit,
};
use veoveo_store::PlatformStore;
use veoveo_types::{ServerSlug, TaskId};
use veoveo_usage::{UsageActor, UsageDraft, UsageStore, UsageVia, UsageWrite};

use crate::{
    contract::*,
    protocol::RuntimeRequest,
    provider::{RuntimeClient, RuntimeError, validate_result},
};

#[derive(Clone, Copy)]
pub enum EmbeddingUsage {
    InputTokens,
}
impl UsageKinds for EmbeddingUsage {
    fn all() -> &'static [Self] {
        &[Self::InputTokens]
    }
    fn declaration(self) -> UsageKindDeclaration {
        UsageKindDeclaration {
            kind: UsageKind::parse("embedding.input_tokens").expect("usage kind"),
            service_category: ServiceCategory::AiAndMachineLearning,
            unit: UsageUnit::parse("Tokens").expect("token unit"),
            monetary: false,
        }
    }
}

pub trait EmbeddingRuntime: Send + Sync {
    fn infer(
        &self,
        request: &RuntimeRequest,
    ) -> impl Future<Output = Result<EmbeddingResult, RuntimeError>> + Send;
    fn device(&self) -> &Device;
}
impl EmbeddingRuntime for RuntimeClient {
    async fn infer(&self, request: &RuntimeRequest) -> Result<EmbeddingResult, RuntimeError> {
        self.embed(request).await
    }
    fn device(&self) -> &Device {
        self.expected_device()
    }
}

#[derive(Clone)]
pub struct MeterEvent {
    pub request_id: uuid::Uuid,
    pub actor: UsageActor,
    pub via: Option<UsageVia>,
    pub at: DateTime<Utc>,
    pub phase: UsagePhase,
    pub tokens: u64,
}
impl MeterEvent {
    pub fn draft(self) -> anyhow::Result<UsageDraft> {
        let mut draft = UsageDraft::builder(
            EmbeddingUsage::InputTokens.declaration().kind,
            self.phase,
            self.actor,
            Quantity::whole(self.tokens),
            UsageSourceId::parse(format!("request:{}", self.request_id))?,
            ChargePeriod::at(self.at),
        );
        if let Some(via) = self.via {
            draft = draft.via(via.server, via.task);
        }
        let draft = draft.build()?;
        Ok(draft)
    }
}
pub trait Meter: Send + Sync {
    fn write(&self, event: MeterEvent) -> impl Future<Output = Result<(), ()>> + Send;
}
#[derive(Clone)]
pub struct CoreMeter {
    platform: PlatformStore,
    ledger: UsageStore,
}
impl CoreMeter {
    pub async fn new(platform: PlatformStore) -> anyhow::Result<Self> {
        let ledger = UsageStore::new(platform.clone());
        let kinds: Vec<_> = EmbeddingUsage::all()
            .iter()
            .map(|kind| kind.declaration())
            .collect();
        ledger.declare_as_server(&kinds).await?;
        Ok(Self { platform, ledger })
    }
}
impl Meter for CoreMeter {
    async fn write(&self, event: MeterEvent) -> Result<(), ()> {
        let phase = event.phase;
        let draft = event.draft().map_err(|_| ())?;
        match phase {
            UsagePhase::Estimate => {
                let mut connection = self.platform.pool().acquire().await.map_err(|_| ())?;
                UsageWrite::reserve(&mut connection, &draft)
                    .await
                    .map_err(|_| ())?;
            }
            UsagePhase::Actual => {
                self.ledger
                    .append_as_server(&[draft])
                    .await
                    .map_err(|_| ())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GenerationError {
    #[error(
        "delegated task identity cannot be represented by native usage accounting; inference not started"
    )]
    UnsupportedAttribution,
    #[error(
        "invalid embedding input or private payload bound; inference not started; no inference usage reserved"
    )]
    InvalidInput,
    #[error("usage reservation failed before inference")]
    ReservationFailed,
    #[error("runtime rejected input before inference ({0}); zero actual usage recorded")]
    NotStarted(RejectionCode),
    #[error("runtime rejected input before inference, but zero-usage accounting failed")]
    RejectionAccountingFailed,
    #[error(
        "runtime execution or output could not be confirmed; conservative usage estimate retained"
    )]
    Ambiguous,
    #[error("inference completed, but actual usage accounting failed; do not automatically replay")]
    InferenceCompletedAccountingFailed,
}

pub async fn generate<R: EmbeddingRuntime, M: Meter>(
    runtime: &R,
    meter: &M,
    request: &RuntimeRequest,
    actor: UsageActor,
    via: Option<UsageVia>,
) -> Result<EmbeddingResult, GenerationError> {
    request
        .validate()
        .map_err(|_| GenerationError::InvalidInput)?;
    let event = MeterEvent {
        request_id: request.request_id,
        actor,
        via,
        at: Utc::now(),
        phase: UsagePhase::Estimate,
        // A conservative bound, never reported as measured tokenizer usage.
        tokens: (request.items.len() as u64 * MAX_ITEM_TOKENS).min(MAX_BATCH_TOKENS),
    };
    meter
        .write(event.clone())
        .await
        .map_err(|_| GenerationError::ReservationFailed)?;
    match runtime.infer(request).await {
        Ok(output) => {
            validate_result(&output, request, runtime.device())
                .map_err(|_| GenerationError::Ambiguous)?;
            meter
                .write(MeterEvent {
                    phase: UsagePhase::Actual,
                    tokens: output.usage.input_tokens,
                    ..event
                })
                .await
                .map_err(|_| GenerationError::InferenceCompletedAccountingFailed)?;
            Ok(output)
        }
        Err(RuntimeError::NotStarted(code)) => {
            meter
                .write(MeterEvent {
                    phase: UsagePhase::Actual,
                    tokens: 0,
                    ..event
                })
                .await
                .map_err(|_| GenerationError::RejectionAccountingFailed)?;
            Err(GenerationError::NotStarted(code))
        }
        Err(_) => Err(GenerationError::Ambiguous),
    }
}

/// Preserve verified Hub delegation using Core's native task parser. No local Task is created.
pub fn usage_via(
    server: Option<&ServerSlug>,
    task: Option<&OpaqueTaskId>,
) -> Result<Option<UsageVia>, GenerationError> {
    match (server, task) {
        (Some(server), task) => Ok(Some(UsageVia {
            server: server.clone(),
            task: task
                .map(|task| TaskId::parse(task.as_str()))
                .transpose()
                .map_err(|_| GenerationError::UnsupportedAttribution)?,
        })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(GenerationError::UnsupportedAttribution),
    }
}
