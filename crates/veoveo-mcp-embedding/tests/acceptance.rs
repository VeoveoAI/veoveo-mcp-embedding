//! Real deployed acceptance, ignored by default. No provider/model calls in ordinary checks.
//! Native Hub/Artifact credentials come from existing environment selectors, never caller signing.
//! VEOVEO_ACCEPTANCE_CASES_FILE is an offline closed manifest with public inputs,
//! fixture recipes/identities, optional token expectations and bounded ranking checks.
//! REPORT must be new; its parent and any baseline must already exist. Reserve and
//! validate every offline input/baseline before any Hub request. Signed transfers are
//! never requested by this runner: safe Artifact resources authorize source identities.
//! Each test makes only its manifest's serial embed calls, with no automatic retry.
//! Credentials, arbitrary private errors and signed storage URLs never enter receipts.
#![cfg(feature = "server")]
#[path = "support/deployed_probe.rs"]
mod deployed_probe;
mod support;
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use support::forwarder::Forward;
use veoveo_hub_sdk::{
    client::{CallError, CatalogError, HubClient, StaticBearer},
    identity::HubProfileId,
};
use veoveo_mcp_artifact::contract::{ArtifactState, ReadArtifact};
use veoveo_mcp_embedding::{
    contract::*,
    provider::{validate_capabilities, validate_public_result},
};
use veoveo_types::{PrincipalId, TenantId, WorkContextId};
const DEADLINE: Duration = Duration::from_secs(65);
const MAX_EVIDENCE_BYTES: usize = 16 * 1024 * 1024;
fn required(name: &str) -> Result<String> {
    std::env::var(format!("VEOVEO_ACCEPTANCE_{name}"))
        .map_err(|_| anyhow!("VEOVEO_ACCEPTANCE_{name} is required"))
}

fn bearer(selector: &str) -> Result<String> {
    ensure!(
        !selector.is_empty()
            && selector.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            }),
        "bearer selector must name an environment variable"
    );
    let value =
        std::env::var(selector).map_err(|_| anyhow!("selected bearer variable is missing"))?;
    ensure!(
        !value.is_empty() && !value.chars().any(char::is_whitespace),
        "selected bearer is invalid"
    );
    Ok(value)
}

struct Installation {
    origin: url::Url,
    profile: HubProfileId,
    http: reqwest::Client,
    member: String,
    restricted: Option<String>,
    principal: PrincipalId,
    tenant: TenantId,
    context: WorkContextId,
    device: Device,
    report: PathBuf,
    baseline: Option<PathBuf>,
}
impl Installation {
    fn load() -> Result<Self> {
        ensure!(
            std::env::var("VEOVEO_EMBEDDING_ACCEPTANCE").as_deref() == Ok("1"),
            "explicit VEOVEO_EMBEDDING_ACCEPTANCE=1 is required for real inference"
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let origin =
            url::Url::parse(&required("HUB")?).map_err(|_| anyhow!("invalid Hub origin"))?;
        ensure!(
            origin.scheme() == "https"
                && origin.host().is_some()
                && origin.path() == "/"
                && origin.username().is_empty()
                && origin.password().is_none()
                && origin.query().is_none()
                && origin.fragment().is_none(),
            "Hub must be one verified HTTPS origin"
        );
        let member = bearer(&required("MEMBER_BEARER_ENV")?)?;
        let restricted = std::env::var("VEOVEO_ACCEPTANCE_READ_ONLY_BEARER_ENV")
            .ok()
            .map(|selector| bearer(&selector))
            .transpose()?;
        ensure!(
            restricted.as_ref() != Some(&member),
            "restricted and member credentials must differ"
        );
        let device = match required("DEVICE")?.as_str() {
            "mps" => Device::Mps,
            "cuda" => Device::Cuda,
            _ => bail!("DEVICE must be mps or cuda"),
        };
        let mut http = reqwest::Client::builder()
            .user_agent("veoveo-reference-qualification/1.0")
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(DEADLINE);
        if let Some(path) = std::env::var_os("VEOVEO_ACCEPTANCE_CA_FILE") {
            let pem = std::fs::read(path).context("cannot read acceptance CA file")?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem)
                .context("invalid acceptance PEM CA bundle")?;
            ensure!(!certificates.is_empty(), "acceptance CA bundle is empty");
            for certificate in certificates {
                http = http.add_root_certificate(certificate);
            }
        }
        Ok(Self {
            origin,
            profile: HubProfileId::parse(
                std::env::var("VEOVEO_ACCEPTANCE_PROFILE").unwrap_or_else(|_| "local".into()),
            )?,
            http: http.build()?,
            member,
            restricted,
            principal: PrincipalId::parse(required("PRINCIPAL")?)?,
            tenant: TenantId::parse(required("TENANT")?)?,
            context: WorkContextId::parse(required("WORK_CONTEXT")?)?,
            device,
            report: PathBuf::from(required("REPORT")?),
            baseline: std::env::var_os("VEOVEO_ACCEPTANCE_BASELINE_REPORT").map(PathBuf::from),
        })
    }
    fn client(&self, token: &str) -> Result<HubClient> {
        Ok(HubClient::new(
            self.origin.join(&format!("/mcp/{}", self.profile))?,
            self.http.clone(),
            StaticBearer::new(token.to_owned()),
        )
        .named(
            "veoveo-embedding-real-acceptance",
            env!("CARGO_PKG_VERSION"),
        ))
    }
    async fn usage(&self) -> Result<Value> {
        let today = chrono::Utc::now().date_naive();
        let mut cursor: Option<String> = None;
        let mut rows = Vec::new();
        for _ in 0..64 {
            let mut url = self
                .origin
                .join(&format!("/admin/{}/usage/daily", self.profile))?;
            {
                let mut query = url.query_pairs_mut();
                query
                    .append_pair("scope", "own")
                    .append_pair("from", &(today - chrono::TimeDelta::days(1)).to_string())
                    .append_pair("to", &(today + chrono::TimeDelta::days(1)).to_string())
                    .append_pair("limit", "1000");
                if let Some(cursor) = &cursor {
                    query.append_pair("cursor", cursor);
                }
            }
            let response = self
                .http
                .get(url)
                .bearer_auth(&self.member)
                .send()
                .await
                .map_err(|_| anyhow!("native own-usage request failed"))?;
            ensure!(
                response.status().is_success(),
                "native own-usage returned non-success status"
            );
            let page: Value = serde_json::from_slice(&bounded_bytes(response).await?)
                .map_err(|_| anyhow!("invalid native own-usage page"))?;
            for row in page["rows"].as_array().context("usage page has no rows")? {
                ensure!(
                    row["principal"] == self.principal.as_str(),
                    "own-usage principal differs from configured caller"
                );
                if row["server"] == "embedding" && row["kind"] == "embedding.input_tokens" {
                    ensure!(
                        row["money"].is_null(),
                        "embedding usage unexpectedly carries money"
                    );
                    rows.push(row.clone());
                }
            }
            match page["nextCursor"].as_str() {
                Some(next) => {
                    ensure!(cursor.as_deref() != Some(next), "usage cursor repeated");
                    cursor = Some(next.to_owned());
                }
                None => return Ok(json!({"rows":rows})),
            }
        }
        bail!("native usage pagination exceeded bound")
    }
}

async fn bounded_bytes(mut response: reqwest::Response) -> Result<Vec<u8>> {
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_EVIDENCE_BYTES as u64),
        "evidence response exceeds byte bound"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("evidence response read failed"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_EVIDENCE_BYTES,
            "evidence response exceeds byte bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    veoveo_types::Sha256Digest::from_bytes(Sha256::digest(bytes).into()).to_string()
}
fn elapsed(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn discovery_error_detail(error: &CallError) -> Value {
    // Initialization failures are already Transport(String) in the pinned SDK.
    // Never recover fields by parsing that string: it can contain private bodies.
    let variant = match error {
        CallError::Refused { .. } => "Refused",
        CallError::Undeclared { .. } => "Undeclared",
        CallError::Rejected { .. } => "Rejected",
        CallError::Tool { .. } => "Tool",
        CallError::Task { .. } => "Task",
        CallError::InputRequired => "InputRequired",
        CallError::Timeout(_) => "Timeout",
        CallError::Decode(_) => "Decode",
        CallError::Content { .. } => "Content",
        CallError::Unsubscribed { .. } => "Unsubscribed",
        CallError::Contract(_) => "Contract",
        CallError::ToolName { .. } => "ToolName",
        CallError::NoCallerBearer => "NoCallerBearer",
        CallError::Catalog(_) => "Catalog",
        CallError::Transport(_) => "Transport",
    };
    let mut detail = json!({"sdkVariant":variant,"httpStatus":null,"jsonRpcCode":null,
        "transportFlags":null,"typedTransportCauseAvailable":false});
    match error {
        CallError::Rejected { code, .. } => detail["jsonRpcCode"] = json!(code),
        CallError::Catalog(error) => {
            detail["catalogVariant"] = json!(match error {
                CatalogError::Request { .. } => "Request",
                CatalogError::RepeatedCursor { .. } => "RepeatedCursor",
                CatalogError::TooManyPages { .. } => "TooManyPages",
                CatalogError::TooManyItems { .. } => "TooManyItems",
                CatalogError::Deadline { .. } => "Deadline",
                CatalogError::Metadata { .. } => "Metadata",
            });
            if let CatalogError::Request { error, .. } = error {
                use rmcp::service::ServiceError;
                detail["serviceVariant"] = json!(match error {
                    ServiceError::McpError(_) => "McpError",
                    ServiceError::TransportSend(_) => "TransportSend",
                    ServiceError::TransportClosed => "TransportClosed",
                    ServiceError::UnexpectedResponse => "UnexpectedResponse",
                    ServiceError::SubscriptionLagged { .. } => "SubscriptionLagged",
                    ServiceError::Cancelled { .. } => "Cancelled",
                    ServiceError::Timeout { .. } => "Timeout",
                    ServiceError::InputRequiredRoundsExceeded { .. } =>
                        "InputRequiredRoundsExceeded",
                    _ => "Other",
                });
                match error {
                    ServiceError::McpError(error) => detail["jsonRpcCode"] = json!(error.code.0),
                    ServiceError::TransportSend(error) => {
                        use rmcp::transport::streamable_http_client::StreamableHttpError;
                        if let Some(error) = error
                            .error
                            .downcast_ref::<StreamableHttpError<reqwest::Error>>()
                        {
                            detail["typedTransportCauseAvailable"] = json!(true);
                            detail["transportVariant"] = json!(match error {
                                StreamableHttpError::Sse(_) => "Sse",
                                StreamableHttpError::Io(_) => "Io",
                                StreamableHttpError::Client(_) => "Client",
                                StreamableHttpError::UnexpectedEndOfStream =>
                                    "UnexpectedEndOfStream",
                                StreamableHttpError::UnexpectedServerResponse(_) =>
                                    "UnexpectedServerResponse",
                                StreamableHttpError::HttpResponse { .. } => "HttpResponse",
                                StreamableHttpError::UnexpectedContentType(_) =>
                                    "UnexpectedContentType",
                                StreamableHttpError::ServerDoesNotSupportSse =>
                                    "ServerDoesNotSupportSse",
                                StreamableHttpError::ServerDoesNotSupportDeleteSession =>
                                    "ServerDoesNotSupportDeleteSession",
                                StreamableHttpError::TokioJoinError(_) => "TokioJoinError",
                                StreamableHttpError::Deserialize(_) => "Deserialize",
                                StreamableHttpError::TransportChannelClosed =>
                                    "TransportChannelClosed",
                                StreamableHttpError::MissingSessionIdInResponse =>
                                    "MissingSessionIdInResponse",
                                StreamableHttpError::AuthRequired(_) => "AuthRequired",
                                StreamableHttpError::InsufficientScope(_) => "InsufficientScope",
                                StreamableHttpError::ReservedHeaderConflict(_) =>
                                    "ReservedHeaderConflict",
                                StreamableHttpError::SessionExpired => "SessionExpired",
                                StreamableHttpError::SessionRecoveryTimeout =>
                                    "SessionRecoveryTimeout",
                                StreamableHttpError::ControlRequestTimeout =>
                                    "ControlRequestTimeout",
                                _ => "Other",
                            });
                            match error {
                                StreamableHttpError::HttpResponse { status, .. } => {
                                    detail["httpStatus"] = json!(status.as_u16())
                                }
                                StreamableHttpError::AuthRequired(_) => {
                                    detail["httpStatus"] = json!(401)
                                }
                                StreamableHttpError::InsufficientScope(_) => {
                                    detail["httpStatus"] = json!(403)
                                }
                                StreamableHttpError::SessionExpired => {
                                    detail["httpStatus"] = json!(404)
                                }
                                StreamableHttpError::Client(error) => {
                                    detail["httpStatus"] =
                                        json!(error.status().map(|status| status.as_u16()));
                                    detail["transportFlags"] = json!({"isConnect":error.is_connect(),
                                        "isTimeout":error.is_timeout(),"isRequest":error.is_request(),
                                        "isBody":error.is_body(),"isDecode":error.is_decode(),
                                        "isRedirect":error.is_redirect(),"isStatus":error.is_status()});
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    detail
}
fn totals(page: &Value, phase: &str) -> Result<(u64, u64)> {
    let mut result = (0_u64, 0_u64);
    for row in page["rows"].as_array().context("usage rows")? {
        if row["phase"] != phase {
            continue;
        }
        let quantity = row["quantity"]
            .as_str()
            .context("quantity is not a native decimal string")?;
        let (whole, fraction) = quantity.split_once('.').unwrap_or((quantity, ""));
        ensure!(
            fraction.bytes().all(|b| b == b'0'),
            "token quantity is fractional"
        );
        result.0 = result
            .0
            .checked_add(whole.parse::<u64>()?)
            .context("token total overflow")?;
        result.1 = result
            .1
            .checked_add(row["records"].as_u64().context("record count")?)
            .context("record total overflow")?;
    }
    Ok(result)
}
fn usage_delta(before: &Value, after: &Value, actual: u64, estimate: u64) -> Result<()> {
    for (phase, tokens) in [("estimate", estimate), ("actual", actual)] {
        let a = totals(before, phase)?;
        let b = totals(after, phase)?;
        ensure!(
            b.0.checked_sub(a.0) == Some(tokens) && b.1.checked_sub(a.1) == Some(1),
            "unexpected {phase} usage delta; concurrent same-principal activity or accounting defect"
        );
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ranking {
    query: usize,
    preferred: usize,
    other: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureRecipe {
    artifact_id: ArtifactId,
    media_type: veoveo_mcp_artifact::contract::MediaType,
    size: veoveo_mcp_artifact::contract::ObjectSize,
    sha256: veoveo_types::Sha256Digest,
    recipe: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    input: EmbedRequest,
    #[serde(default)]
    fixtures: Vec<FixtureRecipe>,
    #[serde(default)]
    expected_tokens: Option<Vec<u64>>,
    #[serde(default)]
    rankings: Vec<Ranking>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    kind: String,
    cases: Vec<Case>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceEvidence {
    part_index: usize,
    source: MediaSource,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    name: String,
    input: EmbedRequest,
    sources: Vec<Vec<SourceEvidence>>,
    output: EmbeddingResult,
    output_bytes: usize,
    output_sha256: String,
    elapsed_ms: f64,
    rankings: Vec<Value>,
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .context("cannot open offline qualification input")?
        .take(MAX_EVIDENCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_EVIDENCE_BYTES,
        "offline qualification input exceeds bound"
    );
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("offline qualification input is not the expected closed JSON shape"))
}
fn validate_manifest(manifest: &Manifest) -> Result<()> {
    ensure!(
        ["corpus", "capacity"].contains(&manifest.kind.as_str()),
        "manifest kind must be corpus or capacity"
    );
    ensure!(
        !manifest.cases.is_empty() && manifest.cases.len() <= 16,
        "manifest must contain1..16cases"
    );
    let mut names = std::collections::BTreeSet::new();
    for case in &manifest.cases {
        ensure!(
            !case.name.is_empty()
                && case.name.len() <= 80
                && case
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
                && names.insert(&case.name),
            "invalid/repeated case name"
        );
        case.input
            .validate()
            .map_err(|_| anyhow!("invalid public acceptance input"))?;
        if let Some(tokens) = &case.expected_tokens {
            ensure!(
                tokens.len() == case.input.items.len()
                    && tokens.iter().all(|n| *n > 0 && *n <= MAX_ITEM_TOKENS)
                    && tokens.iter().sum::<u64>() <= MAX_BATCH_TOKENS,
                "invalid token expectation"
            );
        }
        if manifest.kind == "capacity" {
            ensure!(
                case.expected_tokens
                    .as_ref()
                    .is_some_and(|tokens| tokens.contains(&MAX_ITEM_TOKENS)),
                "capacity requires measured8192-token expectation"
            );
        }
        for ranking in &case.rankings {
            ensure!(
                [ranking.query, ranking.preferred, ranking.other]
                    .iter()
                    .all(|i| *i < case.input.items.len()),
                "ranking index exceeds case"
            );
        }
        for fixture in &case.fixtures {
            ensure!(
                !fixture.recipe.is_empty()
                    && fixture.recipe.len() <= 8192
                    && fixture.size.bytes() > 0
                    && fixture.size.bytes() <= MAX_ARTIFACT_BYTES,
                "invalid fixture recipe"
            );
        }
        for id in case
            .input
            .items
            .iter()
            .flat_map(|item| &item.content)
            .filter_map(ContentPart::artifact_id)
        {
            ensure!(
                case.fixtures
                    .iter()
                    .any(|fixture| fixture.artifact_id == id),
                "each media Artifact ID requires its exact source bytes/hash and fixture recipe"
            );
        }
    }
    Ok(())
}
fn source_rows(sources: &[Vec<SourceEvidence>]) -> Vec<Vec<(usize, MediaSource)>> {
    sources
        .iter()
        .map(|rows| {
            rows.iter()
                .map(|row| (row.part_index, row.source.clone()))
                .collect()
        })
        .collect()
}
fn validate_receipt(receipt: &Receipt) -> Result<()> {
    validate_public_result(
        &receipt.output,
        receipt.output.request_id,
        &receipt.input,
        &source_rows(&receipt.sources),
        &receipt.output.execution.device,
    )?;
    let bytes = serde_json::to_vec(&receipt.output)?;
    ensure!(
        bytes.len() == receipt.output_bytes && digest(&bytes) == receipt.output_sha256,
        "baseline byte evidence differs"
    );
    Ok(())
}
fn baseline(path: Option<&Path>, manifest: &Manifest) -> Result<Option<Vec<Receipt>>> {
    let Some(path) = path else { return Ok(None) };
    let report: Value = read_json(path)?;
    ensure!(report["status"] == "passed", "baseline did not pass");
    let receipts: Vec<Receipt> = serde_json::from_value(report["cases"].clone())
        .map_err(|_| anyhow!("baseline cases are not typed receipts"))?;
    ensure!(
        receipts.len() == manifest.cases.len(),
        "baseline case count differs"
    );
    for (receipt, case) in receipts.iter().zip(&manifest.cases) {
        validate_receipt(receipt)?;
        ensure!(receipt.name == case.name, "baseline case order differs");
        ensure!(
            same_input(&receipt.input, &case.input),
            "baseline semantic input differs"
        );
        for (item_index, item) in case.input.items.iter().enumerate() {
            for (part_index, part) in item.content.iter().enumerate() {
                if let Some(id) = part.artifact_id() {
                    let recipe = case
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.artifact_id == id)
                        .context("baseline recipe missing")?;
                    let source = &receipt.sources[item_index]
                        .iter()
                        .find(|row| row.part_index == part_index)
                        .context("baseline source position missing")?
                        .source;
                    ensure!(
                        source.media_type == recipe.media_type
                            && source.size == recipe.size
                            && source.sha256 == recipe.sha256,
                        "baseline media bytes differ from offline recipe"
                    );
                }
            }
        }
    }
    Ok(Some(receipts))
}
fn same_input(a: &EmbedRequest, b: &EmbedRequest) -> bool {
    a.processing_id().ok() == b.processing_id().ok()
        && a.items.len() == b.items.len()
        && a.items.iter().zip(&b.items).all(|(a, b)| {
            a.content.len() == b.content.len()
                && a.content.iter().zip(&b.content).all(|(a, b)| match (a, b) {
                    (ContentPart::Text { text: a }, ContentPart::Text { text: b }) => a == b,
                    _ => a.modality() == b.modality(),
                })
        })
}
async fn run_with_evidence<I, P, F, Fut>(
    path: &Path,
    preflight: P,
    mut report: Value,
    runner: F,
) -> Result<()>
where
    P: FnOnce() -> Result<I>,
    F: FnOnce(I, Value) -> Fut,
    Fut: Future<Output = (Value, Result<()>)>,
{
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("cannot reserve new report; parent must exist and path must not exist")?;
    let started = Instant::now();
    let outcome = match preflight() {
        Ok(baseline) => {
            let (result, outcome) = runner(baseline, report).await;
            report = result;
            outcome
        }
        Err(error) => Err(error),
    };
    report["status"] = json!(if outcome.is_ok() { "passed" } else { "failed" });
    report["elapsedMs"] = json!(elapsed(started));
    if let Err(error) = &outcome {
        report["failure"] = json!(error.to_string());
    }
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(bytes.len() <= MAX_EVIDENCE_BYTES, "report exceeds bound");
    use std::io::Write as _;
    file.write_all(&bytes)?;
    eprintln!(
        "Embedding acceptance report: {} ({} bytes, {})",
        path.display(),
        bytes.len(),
        digest(&bytes)
    );
    outcome
}
fn record(report: &mut Value, method: &str, started: Instant, detail: Value) {
    let steps = report["sequence"].as_array_mut().unwrap();
    steps.push(json!({"sequence":steps.len()+1,"method":method,"elapsedMs":elapsed(started),"detail":detail}));
}
async fn authorized_sources(client: &HubClient, case: &Case) -> Result<Vec<Vec<SourceEvidence>>> {
    let mut result = vec![];
    for item in &case.input.items {
        let mut rows = vec![];
        for (part_index, part) in item.content.iter().enumerate() {
            if let Some(id) = part.artifact_id() {
                let artifact = client
                    .read(ReadArtifact { artifact: id })
                    .timeout(DEADLINE)
                    .await
                    .map_err(|_| anyhow!("typed Artifact metadata read failed before embedding"))?;
                let artifact = artifact.artifact;
                ensure!(
                    artifact.artifact_id == id && artifact.state == ArtifactState::Available,
                    "Artifact identity/state differs"
                );
                let source = MediaSource {
                    artifact_id: id,
                    media_type: artifact.media_type,
                    size: artifact.size.context("Artifact size unavailable")?,
                    sha256: artifact.sha256.context("Artifact SHA256 unavailable")?,
                };
                let recipe = case
                    .fixtures
                    .iter()
                    .find(|fixture| fixture.artifact_id == id)
                    .context("fixture recipe missing")?;
                ensure!(
                    source.media_type == recipe.media_type
                        && source.size == recipe.size
                        && source.sha256 == recipe.sha256,
                    "Artifact bytes identity differs from fixture recipe"
                );
                rows.push(SourceEvidence { part_index, source });
            }
        }
        result.push(rows);
    }
    Ok(result)
}
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let norm = |v: &[f32]| v.iter().map(|n| f64::from(*n).powi(2)).sum::<f64>().sqrt();
    a.iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>()
        / (norm(a) * norm(b))
}
fn scores(result: &EmbeddingResult, rankings: &[Ranking]) -> Result<Vec<Value>> {
    rankings.iter().map(|ranking|{let query=&result.items[ranking.query].vector;let preferred=cosine(query,&result.items[ranking.preferred].vector);let other=cosine(query,&result.items[ranking.other].vector);ensure!(preferred>other,"real corpus ranking differs from explicit fixture expectation");Ok(json!({"query":ranking.query,"preferred":ranking.preferred,"other":ranking.other,"preferredCosine":preferred,"otherCosine":other}))}).collect()
}
fn parity(previous: &Receipt, current: &Receipt) -> Result<Value> {
    ensure!(
        same_input(&previous.input, &current.input)
            && previous.output.processing_id == current.output.processing_id,
        "parity input/processing differs"
    );
    let (mut cosine_min, mut max_abs) = (1.0_f64, 0.0_f64);
    for (a, b) in previous.output.items.iter().zip(&current.output.items) {
        ensure!(
            a.token_count == b.token_count && a.vector.len() == b.vector.len(),
            "parity token/dimension differs"
        );
        ensure!(a.media.len() == b.media.len(), "parity media count differs");
        for (a, b) in a.media.iter().zip(&b.media) {
            let (a, b) = (a.source(), b.source());
            ensure!(
                a.media_type == b.media_type && a.size == b.size && a.sha256 == b.sha256,
                "parity media bytes differ"
            );
        }
        cosine_min = cosine_min.min(cosine(&a.vector, &b.vector));
        for (a, b) in a.vector.iter().zip(&b.vector) {
            max_abs = max_abs.max((f64::from(*a) - f64::from(*b)).abs());
        }
    }
    ensure!(
        cosine_min >= 0.9999 && max_abs <= 0.0001,
        "cross-device parity exceeds explicit thresholds"
    );
    Ok(
        json!({"pairedCosineMin":cosine_min,"maxAbsoluteDifference":max_abs,"minimumCosine":0.9999,"maximumAbsoluteDifference":0.0001}),
    )
}
async fn run(
    installation: &Installation,
    manifest: &Manifest,
    baseline: Option<&[Receipt]>,
    report: &mut Value,
    composed: bool,
) -> Result<()> {
    let client = installation.client(&installation.member)?;
    let embed_method = if composed {
        "tools/call embeddingprobe__forward_embed"
    } else {
        "tools/call embedding__embed"
    };
    let started = Instant::now();
    let tools = match client.tools().await {
        Ok(tools) => tools,
        Err(error) => {
            record(
                report,
                "tools/list",
                started,
                json!({"status":"failed","nativeSdkError":discovery_error_detail(&error)}),
            );
            bail!("native Hub discovery failed")
        }
    };
    ensure!(
        tools
            .items
            .iter()
            .any(|tool| tool.name == "embedding__embed"),
        "embedding absent from native discovery"
    );
    ensure!(
        !composed
            || tools
                .items
                .iter()
                .any(|tool| tool.name == "embeddingprobe__forward_embed"),
        "registered composed probe absent from native discovery"
    );
    record(
        report,
        "tools/list",
        started,
        json!({"embeddingAdvertised":true,"composedProbeAdvertised":composed,"catalogComplete":tools.is_complete()}),
    );
    let started = Instant::now();
    let capabilities = client
        .read(ReadCapabilities)
        .timeout(DEADLINE)
        .await
        .map_err(|_| anyhow!("native capabilities read failed"))?;
    validate_capabilities(&capabilities, &installation.device)?;
    report["capabilitiesStartupSnapshot"] = serde_json::to_value(capabilities)?;
    record(
        report,
        "resources/read embedding://capabilities",
        started,
        json!({"validated":true,"freshHealthClaim":false}),
    );
    for (index, case) in manifest.cases.iter().enumerate() {
        let started = Instant::now();
        let sources = authorized_sources(&client, case).await?;
        record(
            report,
            "resources/read Artifact identities",
            started,
            json!({"case":case.name,"mediaParts":sources.iter().map(Vec::len).sum::<usize>()}),
        );
        let before = installation.usage().await?;
        let started = Instant::now();
        let output = if composed { client.call(Forward{request:case.input.clone()}).timeout(DEADLINE).await }
            else { client.call(case.input.clone()).timeout(DEADLINE).await }
            .map_err(|error|{record(report,"typed embed call",started,json!({"case":case.name,"status":"failed","nativeSdkError":discovery_error_detail(&error)}));anyhow!("embedding call failed; no automatic replay")})?;
        let measured = elapsed(started);
        validate_public_result(
            &output,
            output.request_id,
            &case.input,
            &source_rows(&sources),
            &installation.device,
        )?;
        let bytes = serde_json::to_vec(&output)?;
        let receipt = Receipt {
            name: case.name.clone(),
            input: case.input.clone(),
            sources,
            output,
            output_bytes: bytes.len(),
            output_sha256: digest(&bytes),
            elapsed_ms: measured,
            rankings: vec![],
        };
        // Preserve expensive validated observations before quality, accounting or
        // parity checks: a later failure must not erase the actual result.
        let observed = report["cases"].as_array().unwrap().len();
        report["cases"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(&receipt)?);
        record(
            report,
            embed_method,
            started,
            json!({"case":case.name,"requestId":receipt.output.request_id,"embedElapsedMs":measured,
            "outputBytes":receipt.output_bytes,"outputSha256":receipt.output_sha256,"validation":"passed"}),
        );
        if let Some(expected) = &case.expected_tokens {
            ensure!(
                receipt
                    .output
                    .items
                    .iter()
                    .map(|item| item.token_count)
                    .collect::<Vec<_>>()
                    == *expected,
                "actual tokens differ from preflight expectation"
            );
        }
        let rankings = scores(&receipt.output, &case.rankings)?;
        report["cases"][observed]["rankings"] = serde_json::to_value(rankings)?;
        let after = installation.usage().await?;
        record(
            report,
            "observe own usage after inference",
            Instant::now(),
            json!({"case":case.name,"before":before,"after":after}),
        );
        usage_delta(
            &before,
            &after,
            receipt.output.usage.input_tokens,
            (case.input.items.len() as u64 * MAX_ITEM_TOKENS).min(MAX_BATCH_TOKENS),
        )?;
        let comparison = baseline
            .map(|baseline| parity(&baseline[index], &receipt))
            .transpose()?;
        record(
            report,
            "post-inference accounting and parity",
            started,
            json!({"case":case.name,"requestId":receipt.output.request_id,"outputBytes":receipt.output_bytes,"outputSha256":receipt.output_sha256,"usageBefore":before,"usageAfter":after,"parity":comparison}),
        );
    }
    if let Some(restricted) = &installation.restricted {
        let client = installation.client(restricted)?;
        let before = installation.usage().await?;
        let started = Instant::now();
        let denied = client
            .call(manifest.cases[0].input.clone())
            .timeout(DEADLINE)
            .await;
        ensure!(
            denied.is_err(),
            "restricted identity unexpectedly computed embeddings"
        );
        ensure!(
            installation.usage().await? == before,
            "denied call changed own usage"
        );
        record(
            report,
            "restricted identity embed",
            started,
            json!({"denied":true,"memberUsageUnchanged":true}),
        );
    } else {
        report["restrictedIdentityProbe"] =
            json!("unavailable: no pre-existing restricted identity selected");
    }
    Ok(())
}
async fn qualify(kind: &str, composed: bool) -> Result<()> {
    let installation = Installation::load()?;
    let case_path = PathBuf::from(required("CASES_FILE")?);
    let report = json!({"scenario":kind,"composition":composed,"server":"embedding","device":installation.device,"tenant":installation.tenant,"principal":installation.principal,"workContext":installation.context,
        "sequence":[],"cases":[],"outputByteEvidence":"compact structured-result reserialization, not raw HTTP wire","ownUsageProofLimit":"daily own usage proves selected principal totals; full request/delegation attribution needs native Core/Hub evidence"});
    let inspection = &installation;
    run_with_evidence(
        &installation.report,
        || {
            let manifest: Manifest = read_json(&case_path)?;
            validate_manifest(&manifest)?;
            ensure!(
                manifest.kind == kind,
                "manifest kind differs from selected test"
            );
            ensure!(
                !composed || manifest.cases.len() == 1,
                "bounded composed probe takes exactly one case"
            );
            let baseline = baseline(installation.baseline.as_deref(), &manifest)?;
            let probe = if composed {
                Some(deployed_probe::ProbeConfig::load()?)
            } else {
                None
            };
            Ok((manifest, baseline, probe))
        },
        report,
        |(manifest, baseline, probe), mut report| async move {
            report["fixtureRecipes"] = json!(
                manifest
                    .cases
                    .iter()
                    .map(|case| json!({"case":case.name,"fixtures":case.fixtures}))
                    .collect::<Vec<_>>()
            );
            let outcome = if let Some(probe) = probe {
                probe
                    .run(inspection, |readiness| {
                        report["compositionReadiness"] = json!(readiness);
                        run(
                            inspection,
                            &manifest,
                            baseline.as_deref(),
                            &mut report,
                            true,
                        )
                    })
                    .await
            } else {
                run(
                    inspection,
                    &manifest,
                    baseline.as_deref(),
                    &mut report,
                    false,
                )
                .await
            };
            (report, outcome)
        },
    )
    .await
}
#[tokio::test]
#[ignore = "requires explicit native Hub, external media Artifact corpus and real GPU runtime"]
async fn native_hub_real_embedding_corpus_and_usage() -> Result<()> {
    qualify("corpus", false).await
}
#[tokio::test]
#[ignore = "requires measured token recipes in external capacity manifest and real GPU runtime"]
async fn native_hub_real_embedding_capacity_and_usage() -> Result<()> {
    qualify("capacity", false).await
}
#[tokio::test]
#[ignore = "requires explicitly admitted temporary native registration/endpoint/custody; one real composed case"]
async fn native_hub_registered_composition_and_usage() -> Result<()> {
    qualify("corpus", true).await
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DenialPreparation {
    artifact_id: ArtifactId,
    owner: PrincipalId,
    work_context: WorkContextId,
    preparer: PrincipalId,
    evidence_sha256: veoveo_types::Sha256Digest,
    /// External owner/preparer confirms existence and no selected-caller read ACL.
    /// Different ownership within the same Work Context is insufficient.
    caller_has_no_read_access: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum NegativeExpectation {
    RuntimeNotStarted { code: RejectionCode },
    ArtifactDenied { prepared: Option<DenialPreparation> },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NegativeCase {
    name: String,
    input: EmbedRequest,
    #[serde(default)]
    fixtures: Vec<FixtureRecipe>,
    expect: NegativeExpectation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NegativeManifest {
    kind: String,
    cases: Vec<NegativeCase>,
}
fn negative_preflight(manifest: &NegativeManifest) -> Result<()> {
    ensure!(
        manifest.kind == "negative" && !manifest.cases.is_empty() && manifest.cases.len() <= 8,
        "negative manifest requires 1..8 cases"
    );
    let mut names = std::collections::BTreeSet::new();
    for case in &manifest.cases {
        ensure!(
            !case.name.is_empty()
                && case.name.len() <= 80
                && case
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
                && names.insert(&case.name),
            "invalid/repeated negative case name"
        );
        case.input
            .validate()
            .map_err(|_| anyhow!("invalid public negative input"))?;
        match &case.expect {
            NegativeExpectation::RuntimeNotStarted { .. } => validate_manifest(&Manifest {
                kind: "corpus".into(),
                cases: vec![Case {
                    name: case.name.clone(),
                    input: case.input.clone(),
                    fixtures: case.fixtures.clone(),
                    expected_tokens: None,
                    rankings: vec![],
                }],
            })?,
            NegativeExpectation::ArtifactDenied { prepared } => {
                let ids = case
                    .input
                    .items
                    .iter()
                    .flat_map(|item| &item.content)
                    .filter_map(ContentPart::artifact_id)
                    .collect::<Vec<_>>();
                ensure!(
                    ids.len() == 1 && case.fixtures.is_empty(),
                    "ACL denial requires exactly one reference, no readable source recipe"
                );
                if let Some(proof) = prepared {
                    ensure!(
                        proof.artifact_id == ids[0] && proof.caller_has_no_read_access,
                        "owner/preparer must confirm existing Artifact outside caller ACL"
                    );
                }
            }
        }
    }
    Ok(())
}
/// The SDK erases ErrorData.data here. Match only this service's complete closed
/// generated text in test code; never emit arbitrary SDK error messages.
fn negative_request_id(error: &CallError, expected: &NegativeExpectation) -> Result<uuid::Uuid> {
    let message = match error {
        CallError::Rejected { message, .. } | CallError::Tool { message } => message.as_str(),
        _ => bail!("negative call did not produce a closed tool rejection"),
    };
    let rest = message
        .strip_prefix("embedding request ")
        .context("not a closed embedding rejection")?;
    let (id, suffix) = rest
        .split_once(": ")
        .context("not a closed embedding rejection")?;
    let id =
        uuid::Uuid::parse_str(id).map_err(|_| anyhow!("invalid rejection request identity"))?;
    ensure!(id.get_version_num() == 7, "rejection request is not UUIDv7");
    let expected=match expected {
        NegativeExpectation::RuntimeNotStarted{code} if code.is_input_error()=>format!("{code}; inference not started; zero actual usage recorded"),
        NegativeExpectation::RuntimeNotStarted{code}=>format!("runtime rejected input before inference ({code}); zero actual usage recorded"),
        NegativeExpectation::ArtifactDenied{..}=>"artifact authorization or metadata is unavailable; inference not started; no inference usage reserved".into(),
    };
    ensure!(
        suffix == expected,
        "closed rejection differs from expected classification"
    );
    Ok(id)
}
async fn run_negative(
    installation: &Installation,
    manifest: &NegativeManifest,
    report: &mut Value,
) -> Result<()> {
    let client = installation.client(&installation.member)?;
    for case in &manifest.cases {
        if matches!(
            case.expect,
            NegativeExpectation::ArtifactDenied { prepared: None }
        ) {
            report["cases"].as_array_mut().unwrap().push(json!({"name":case.name,"status":"not_exercised","reason":"no existing restricted Artifact with owner/preparer evidence; runner creates no identity or grants"}));
            continue;
        }
        if let NegativeExpectation::RuntimeNotStarted { .. } = case.expect {
            let sources = authorized_sources(
                &client,
                &Case {
                    name: case.name.clone(),
                    input: case.input.clone(),
                    fixtures: case.fixtures.clone(),
                    expected_tokens: None,
                    rankings: vec![],
                },
            )
            .await?;
            report["sourceEvidence"][&case.name] = serde_json::to_value(sources)?;
        }
        if let NegativeExpectation::ArtifactDenied {
            prepared: Some(proof),
        } = &case.expect
        {
            // A native read refusal is checked in addition to the external proof
            // of existence/ACL. An unknown ID alone never proves ACL denial.
            let denial = client
                .read(ReadArtifact {
                    artifact: proof.artifact_id,
                })
                .timeout(DEADLINE)
                .await;
            ensure!(
                matches!(
                    denial,
                    Err(CallError::Rejected { .. } | CallError::Tool { .. })
                ),
                "prepared Artifact was readable or read did not produce a native refusal"
            );
            report["preparationEvidence"][&case.name] = serde_json::to_value(proof)?;
        }
        let before = installation.usage().await?;
        let started = Instant::now();
        let error = client
            .call(case.input.clone())
            .timeout(DEADLINE)
            .await
            .err()
            .context("negative input unexpectedly completed inference")?;
        let id = negative_request_id(&error, &case.expect)?;
        report["cases"].as_array_mut().unwrap().push(json!({"name":case.name,"requestId":id,"expectation":case.expect,"elapsedMs":elapsed(started),"classification":"confirmed_not_started"}));
        let after = installation.usage().await?;
        record(
            report,
            "typed negative embed and own usage",
            started,
            json!({"case":case.name,"requestId":id,"before":before,"after":after}),
        );
        match case.expect {
            NegativeExpectation::RuntimeNotStarted { .. } => usage_delta(
                &before,
                &after,
                0,
                (case.input.items.len() as u64 * MAX_ITEM_TOKENS).min(MAX_BATCH_TOKENS),
            )?,
            NegativeExpectation::ArtifactDenied { .. } => ensure!(
                before == after,
                "Artifact denial reserved or recorded inference usage"
            ),
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires real malformed media Artifact and externally confirmed existing denied Artifact (or explicit not-exercised case)"]
async fn native_hub_real_embedding_rejections_and_artifact_denial() -> Result<()> {
    let installation = Installation::load()?;
    let path = PathBuf::from(required("CASES_FILE")?);
    let inspection = &installation;
    let report = json!({"scenario":"negative","sequence":[],"cases":[],"sourceEvidence":{},"preparationEvidence":{},"aclProofLimit":"existing object/ACL confirmed by external owner/preparer; native read and embedding resolution refusal observed separately"});
    run_with_evidence(
        &installation.report,
        || {
            let manifest: NegativeManifest = read_json(&path)?;
            negative_preflight(&manifest)?;
            Ok(manifest)
        },
        report,
        |manifest, mut report| async move {
            let outcome = run_negative(inspection, &manifest, &mut report).await;
            (report, outcome)
        },
    )
    .await
}
#[test]
fn closed_negative_errors_do_not_accept_missing_ids_or_arbitrary_private_messages() {
    let id = uuid::Uuid::now_v7();
    let expected = NegativeExpectation::RuntimeNotStarted {
        code: RejectionCode::InvalidMedia,
    };
    let error = CallError::Tool {
        message: format!(
            "embedding request {id}: invalid_media; inference not started; zero actual usage recorded"
        ),
    };
    assert_eq!(negative_request_id(&error, &expected).unwrap(), id);
    let expected = NegativeExpectation::RuntimeNotStarted {
        code: RejectionCode::PreprocessingFailed,
    };
    let error = CallError::Tool {
        message: format!(
            "embedding request {id}: runtime rejected input before inference (preprocessing_failed); zero actual usage recorded"
        ),
    };
    assert_eq!(negative_request_id(&error, &expected).unwrap(), id);

    let error = CallError::Tool {
        message: "private arbitrary body with signed URL".into(),
    };
    assert!(negative_request_id(&error, &expected).is_err());
    let mut input = support::input();
    input.items[0].content = vec![ContentPart::Image {
        artifact_id: ArtifactId::parse(uuid::Uuid::now_v7().to_string()).unwrap(),
    }];
    let mut manifest = NegativeManifest {
        kind: "negative".into(),
        cases: vec![NegativeCase {
            name: "denied".into(),
            input,
            fixtures: vec![],
            expect: NegativeExpectation::ArtifactDenied { prepared: None },
        }],
    };
    negative_preflight(&manifest).unwrap();
    manifest.cases[0].expect = NegativeExpectation::ArtifactDenied {
        prepared: Some(DenialPreparation {
            artifact_id: ArtifactId::parse(uuid::Uuid::now_v7().to_string()).unwrap(),
            owner: PrincipalId::parse("00000000-0000-7000-8000-000000000001").unwrap(),
            work_context: WorkContextId::parse("00000000-0000-7000-8000-000000000002").unwrap(),
            preparer: PrincipalId::parse("00000000-0000-7000-8000-000000000003").unwrap(),
            evidence_sha256: veoveo_types::Sha256Digest::from_bytes([0; 32]),
            caller_has_no_read_access: true,
        }),
    };
    assert!(negative_preflight(&manifest).is_err());
}

#[tokio::test]
async fn evidence_preflight_prevents_live_entry_and_preserves_failed_receipts() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "embedding-evidence-preflight-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir(&directory)?;
    let path = directory.join("receipt.json");
    let called = std::sync::atomic::AtomicBool::new(false);
    let result = run_with_evidence(
        &path,
        || Err::<(), _>(anyhow!("synthetic offline preflight failure")),
        json!({"sequence":[],"cases":[]}),
        |_, report| async {
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            (report, Ok(()))
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    let receipt: Value = read_json(&path)?;
    assert_eq!(receipt["status"], "failed");
    let existing = std::fs::read(&path)?;
    assert!(
        run_with_evidence(
            &path,
            || Ok(()),
            json!({}),
            |_, report| async { (report, Ok(())) }
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(&path)?, existing);
    // A later quality/accounting failure still preserves previously observed output.
    let later = directory.join("later.json");
    let input = support::input();
    let output = support::output(&support::text_request(&input));
    let result = run_with_evidence(
        &later,
        || Ok(()),
        json!({"sequence":[],"cases":[]}),
        |_, mut report| async move {
            report["cases"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::to_value(&output).unwrap());
            (
                report,
                Err(anyhow!("synthetic post-inference quality failure")),
            )
        },
    )
    .await;
    assert!(result.is_err());
    let receipt: Value = read_json(&later)?;
    assert_eq!(receipt["cases"][0]["items"][0]["vector"][0], 1.0);
    assert_eq!(receipt["status"], "failed");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn parity_enforces_the_prescoring_component_threshold_even_when_cosine_passes() {
    let input = support::input();
    let output = support::output(&support::text_request(&input));
    let previous = Receipt {
        name: "synthetic-threshold".into(),
        input,
        sources: vec![vec![]],
        output,
        output_bytes: 0,
        output_sha256: String::new(),
        elapsed_ms: 0.0,
        rankings: vec![],
    };
    let mut current = previous.clone();
    current.output.items[0].vector[1] = 0.00005;
    assert!(parity(&previous, &current).is_ok());
    current.output.items[0].vector[1] = 0.0002;
    assert!(
        cosine(
            &previous.output.items[0].vector,
            &current.output.items[0].vector
        ) >= 0.9999
    );
    assert!(parity(&previous, &current).is_err());
}
