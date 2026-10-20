//! Synthetic provider vectors test orchestration/accounting, never model inference.
#![cfg(feature = "server")]
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use veoveo_mcp_embedding::{
    contract::*, protocol::RuntimeRequest, provider::RuntimeError, usage::*,
};
mod support;
use support::{output, request};
use veoveo_types::{PrincipalId, TenantId, WorkContextId};
use veoveo_usage::{UsageActor, UsagePhase};

fn actor() -> UsageActor {
    UsageActor {
        tenant: TenantId::parse("tenant-a").unwrap(),
        principal: PrincipalId::parse("https://idp.example.com#user-1").unwrap(),
        work_context: WorkContextId::parse("mission").unwrap(),
    }
}
struct Synthetic {
    mode: u8,
    calls: AtomicUsize,
}
impl EmbeddingRuntime for Synthetic {
    async fn infer(&self, request: &RuntimeRequest) -> Result<EmbeddingResult, RuntimeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            0 => Ok(output(request)),
            1 => Err(RuntimeError::NotStarted(RejectionCode::ItemTokenLimit)),
            2 => Err(RuntimeError::Ambiguous),
            _ => {
                let mut value = output(request);
                value.items[0].vector[0] = f32::NAN;
                Ok(value)
            }
        }
    }
    fn device(&self) -> &Device {
        &Device::Mps
    }
}
#[derive(Default)]
struct RecordingMeter {
    events: Mutex<Vec<MeterEvent>>,
    fail_at: Option<usize>,
}
impl Meter for RecordingMeter {
    async fn write(&self, event: MeterEvent) -> Result<(), ()> {
        let mut events = self.events.lock().unwrap();
        if self.fail_at == Some(events.len()) {
            return Err(());
        }
        events.push(event);
        Ok(())
    }
}
#[tokio::test]
async fn measured_actuals_zero_rejections_and_ambiguous_estimates_stay_distinct() {
    for mode in 0..=3 {
        let runtime = Synthetic {
            mode,
            calls: AtomicUsize::new(0),
        };
        let meter = RecordingMeter::default();
        let request = request();
        let result = generate(&runtime, &meter, &request, actor(), None).await;
        let records = meter.events.lock().unwrap();
        assert_eq!(records[0].phase, UsagePhase::Estimate);
        assert_eq!(records[0].tokens, MAX_ITEM_TOKENS);
        assert_eq!(records[0].actor.tenant.as_str(), "tenant-a");
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 1, "no hidden replay");
        match mode {
            0 => {
                assert!(result.is_ok());
                assert_eq!(records[1].tokens, 12);
            }
            1 => {
                assert!(matches!(
                    result,
                    Err(GenerationError::NotStarted(RejectionCode::ItemTokenLimit))
                ));
                assert_eq!(records[1].tokens, 0);
            }
            _ => {
                assert!(matches!(result, Err(GenerationError::Ambiguous)));
                assert_eq!(records.len(), 1);
            }
        }
    }
}
#[tokio::test]
async fn accounting_failure_never_reports_unqualified_success_or_replays_inference() {
    for (fail_at, expected_calls) in [(0, 0), (1, 1)] {
        let runtime = Synthetic {
            mode: 0,
            calls: AtomicUsize::new(0),
        };
        let meter = RecordingMeter {
            fail_at: Some(fail_at),
            ..Default::default()
        };
        let result = generate(&runtime, &meter, &request(), actor(), None).await;
        assert_eq!(runtime.calls.load(Ordering::SeqCst), expected_calls);
        if fail_at == 0 {
            assert!(matches!(result, Err(GenerationError::ReservationFailed)));
        } else {
            assert!(matches!(
                result,
                Err(GenerationError::InferenceCompletedAccountingFailed)
            ));
        }
    }
}
#[test]
fn output_boundary_refuses_partial_wrong_order_model_tokens_dimensions_norm_and_nonfinite() {
    use veoveo_mcp_embedding::provider::validate_result;
    let request = request();
    for change in 0..7 {
        let mut value = output(&request);
        match change {
            0 => value.items.clear(),
            1 => value.items[0].index = 1,
            2 => value.model.revision = "wrong".into(),
            3 => value.items[0].vector.pop().map(|_| ()).unwrap(),
            4 => value.items[0].vector[0] = 0.,
            5 => value.items[0].vector[0] = f32::INFINITY,
            _ => value.usage.input_tokens += 1,
        }
        assert!(validate_result(&value, &request, &Device::Mps).is_err());
    }
}
#[tokio::test]
async fn native_core_ledger_accepts_non_task_nonmonetary_usage_through_server_login() {
    use veoveo_fixtures::store::TestDb;
    use veoveo_usage::UsageStore;
    let db = TestDb::new().await;
    let store = db.server_store(&veoveo_mcp_embedding::server()).await;
    let meter = CoreMeter::new(store.clone()).await.unwrap();
    let runtime = Synthetic {
        mode: 0,
        calls: AtomicUsize::new(0),
    };
    generate(&runtime, &meter, &request(), actor(), None)
        .await
        .unwrap();
    let today = chrono::Utc::now().date_naive();
    let rows = UsageStore::new(store)
        .daily(&actor().tenant, today, today.succ_opt().unwrap(), 10)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.money.is_none() && row.kind.as_str() == "embedding.input_tokens")
    );
    assert!(
        rows.iter().any(|row| row.phase == UsagePhase::Actual
            && row.quantity == veoveo_usage::Quantity::whole(12))
    );
}

#[tokio::test]
async fn delegated_usage_preserves_via_without_creating_an_embedding_task() {
    use veoveo_usage::UsageVia;
    let via = UsageVia {
        server: veoveo_types::ServerSlug::parse("caller").unwrap(),
        task: Some(veoveo_types::TaskId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap()),
    };
    let runtime = Synthetic {
        mode: 0,
        calls: AtomicUsize::new(0),
    };
    let meter = RecordingMeter::default();
    generate(&runtime, &meter, &request(), actor(), Some(via.clone()))
        .await
        .unwrap();
    let records = meter.events.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|event| event.via.as_ref() == Some(&via)));
}

#[test]
fn unsupported_opaque_delegated_tasks_are_rejected_without_losing_attribution() {
    let server = veoveo_types::ServerSlug::parse("caller").unwrap();
    let opaque = veoveo_hub_sdk::OpaqueTaskId::parse("native-task-not-a-uuid").unwrap();
    assert!(matches!(
        usage_via(Some(&server), Some(&opaque)),
        Err(GenerationError::UnsupportedAttribution)
    ));
    assert!(matches!(
        usage_via(None, Some(&opaque)),
        Err(GenerationError::UnsupportedAttribution)
    ));
    let task = veoveo_hub_sdk::OpaqueTaskId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
    let via = usage_via(Some(&server), Some(&task)).unwrap().unwrap();
    let event = MeterEvent {
        request_id: uuid::Uuid::now_v7(),
        actor: actor(),
        via: Some(via),
        at: chrono::Utc::now(),
        phase: UsagePhase::Actual,
        tokens: 12,
    };
    let row = event.draft().unwrap().to_row();
    assert_eq!(row["acting_server"], "caller");
    assert_eq!(row["acting_task"], task.as_str());
    assert!(
        row.get("task_id").is_none(),
        "embedding never creates a Task"
    );
}
