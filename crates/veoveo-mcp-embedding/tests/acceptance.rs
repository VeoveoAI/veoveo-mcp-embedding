//! Real deployed acceptance, ignored by default. No synthetic inference or caller signing.
//!
//! Explicit run: VEOVEO_EMBEDDING_ACCEPTANCE=1; VEOVEO_ACCEPTANCE_HUB is the native
//! HTTPS Hub origin, PROFILE defaults to local. MEMBER_BEARER_ENV names an existing
//! private bearer/opaque-reference environment variable. TENANT, PRINCIPAL,
//! WORK_CONTEXT, DEVICE (mps/cuda) and REPORT are required under VEOVEO_ACCEPTANCE_.
//! Optional READ_ONLY_BEARER_ENV selects an existing restricted identity; absence is
//! reported as unavailable, never provisioned. Optional BASELINE_REPORT selects a
//! prior report from this runner for Mac/Linux parity. No IdP/Hub issuer equivalence
//! is assumed. Optional VEOVEO_ACCEPTANCE_CA_FILE appends a PEM CA bundle to normal
//! certificate trust, retaining hostname verification (local gateway CA only).
//! Run serially with other embedding calls for this principal paused:
//! cargo nextest run --test acceptance --run-ignored only --test-threads 1
//! Select `native_hub_real_embedding_capacity_and_usage` explicitly for the two
//! token-ceiling workloads; it uses a separate REPORT and performs two real calls.
//!
//! Reports intentionally contain the fixed public corpus and real vectors, as
//! qualification evidence only. They never contain credentials, arbitrary error
//! bodies, signed URLs or service state. Byte hashes cover compact reserialized
//! structured tool output, not an uninstrumented HTTP wire body.
#![cfg(feature = "server")]

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use veoveo_hub_sdk::{
    client::{CallError, HubClient, StaticBearer},
    identity::HubProfileId,
};
use veoveo_mcp_embedding::{
    contract::*,
    provider::{validate_capabilities, validate_result},
};
use veoveo_types::{PrincipalId, TenantId, WorkContextId};

const DEADLINE: Duration = Duration::from_secs(65);
const MAX_EVIDENCE_BYTES: usize = 2 * 1024 * 1024;

fn corpus() -> EmbedRequest {
    // Final deployed corpus uses actual newlines in code documents. Its Mac report
    // is the Linux baseline; earlier direct-runtime receipts are not numeric baselines.
    serde_json::from_value(json!({"items":[
        {"purpose":"search_query","text":"Which planet is known as the Red Planet?"},
        {"purpose":"document","title":"Mars","content":"Mars is called the Red Planet because iron oxide on its surface gives it a reddish appearance."},
        {"purpose":"document","title":"Venus","content":"Venus is the hottest planet, with a dense carbon dioxide atmosphere and no red iron oxide surface."},
        {"purpose":"code_query","text":"Find Python code that adds two integers."},
        {"purpose":"document","title":"addition function","content":"def add(a, b):\n    return a + b"},
        {"purpose":"document","title":"multiplication function","content":"def multiply(a, b):\n    return a * b"}
    ]})).expect("fixed public corpus")
}

struct CapacityCase {
    name: &'static str,
    input: EmbedRequest,
    recipe: Value,
    expected_tokens: Vec<u64>,
}

fn capacity_cases() -> [CapacityCase; 2] {
    // Exact public items from runtime-token-ceiling-{2x8192,
    // mixed-1x8192-plus-7short}-request-2668a9.json. Runtime request IDs are
    // deliberately omitted: the native MCP assigns each fresh request ID.
    let long = |title: &str| EmbeddingInput::Document {
        content: "a ".repeat(8180),
        title: Some(title.into()),
    };
    let mut mixed = vec![long("Token ceiling document A")];
    mixed.extend((1..=7).map(|index| EmbeddingInput::Document {
        content: format!("short input item {index}"),
        title: Some(format!("short-{index}")),
    }));
    let long_recipe = json!({"purpose":"document","content":{"unit":"a ","repeat":8180,
        "utf8Bytes":16360,"sha256":"sha256:20c36711eccc025c3c2e6f2b4ea898ef38e675546a927a41f647bd9f4ea9bb62"}});
    [
        CapacityCase {
            name: "2x8192",
            input: EmbedRequest {
                items: vec![
                    long("Token ceiling document A"),
                    long("Token ceiling document B"),
                ],
            },
            recipe: json!({"longDocument":long_recipe,
                "orderedTitles":["Token ceiling document A","Token ceiling document B"]}),
            expected_tokens: vec![8192, 8192],
        },
        CapacityCase {
            name: "mixed-1x8192-plus-7short",
            input: EmbedRequest { items: mixed },
            recipe: json!({"first":{"document":long_recipe,"title":"Token ceiling document A"},
                "then":{"indicesInclusive":[1,7],"purpose":"document",
                    "titleFormat":"short-{index}","contentFormat":"short input item {index}"}}),
            expected_tokens: vec![8192, 15, 15, 15, 15, 15, 15, 15],
        },
    ]
}

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
fn record(report: &mut Value, method: &str, started: Instant, detail: Value) {
    let steps = report["sequence"].as_array_mut().expect("sequence");
    steps.push(json!({"sequence":steps.len()+1,"method":method,"elapsedMs":elapsed(started),"detail":detail}));
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
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    dot / (norm(a) * norm(b))
}
fn semantic_scores(result: &EmbeddingResult) -> Result<Value> {
    ensure!(
        result.items.len() == 6,
        "semantic corpus must have six ordered results"
    );
    let text = [
        cosine(&result.items[0].vector, &result.items[1].vector),
        cosine(&result.items[0].vector, &result.items[2].vector),
    ];
    let code = [
        cosine(&result.items[3].vector, &result.items[4].vector),
        cosine(&result.items[3].vector, &result.items[5].vector),
    ];
    ensure!(
        text[0] > text[1] && code[0] > code[1],
        "real text/code rankings differ from expected corpus rankings"
    );
    Ok(json!({"mars":text[0],"venus":text[1],"addition":code[0],"multiplication":code[1]}))
}
fn parity(reference: &EmbeddingResult, current: &EmbeddingResult) -> Result<Value> {
    ensure!(
        reference.model == current.model
            && reference.processing_id == current.processing_id
            && reference.items.len() == current.items.len(),
        "parity semantic identity/count differs"
    );
    let mut min_cosine = 1_f64;
    let mut max_abs = 0_f64;
    for (a, b) in reference.items.iter().zip(&current.items) {
        ensure!(
            a.index == b.index
                && a.purpose == b.purpose
                && a.token_count == b.token_count
                && a.vector.len() == DIMENSIONS
                && b.vector.len() == DIMENSIONS,
            "parity order/tokens/dimensions differ"
        );
        min_cosine = min_cosine.min(cosine(&a.vector, &b.vector));
        for (x, y) in a.vector.iter().zip(&b.vector) {
            max_abs = max_abs.max((f64::from(*x) - f64::from(*y)).abs());
        }
    }
    ensure!(
        min_cosine >= 0.9999 && max_abs <= 0.001,
        "cross-device parity exceeds declared thresholds"
    );
    semantic_scores(reference)?;
    semantic_scores(current)?;
    Ok(
        json!({"pairedCosineMin":min_cosine,"maxAbsoluteDifference":max_abs,"minimumCosine":0.9999,"maximumAbsoluteDifference":0.001}),
    )
}

async fn run(installation: &Installation, report: &mut Value) -> Result<()> {
    let client = installation.client(&installation.member)?;
    let started = Instant::now();
    let tools = client
        .tools()
        .await
        .map_err(|_| anyhow!("native Hub discovery failed"))?;
    record(
        report,
        "tools/list",
        started,
        json!({"embeddingAdvertised":tools.items.iter().any(|tool| tool.name == "embedding__embed"),
            "catalogComplete":tools.is_complete(),"toolCount":tools.items.len(),
            "toolNames":tools.items.iter().take(256).map(|tool| tool.name.as_ref()).collect::<Vec<_>>(),
            "toolNamesTruncated":tools.items.len()>256,
            "degradation":tools.degradation.failures.iter().take(256).map(|failure|
                json!({"server":failure.server,"surface":failure.surface,"code":failure.code})).collect::<Vec<_>>(),
            "degradationTruncated":tools.degradation.failures.len()>256}),
    );
    ensure!(
        tools
            .items
            .iter()
            .any(|tool| tool.name == "embedding__embed"),
        "embedding tool absent from authenticated discovery"
    );
    let started = Instant::now();
    let capabilities = client
        .read(ReadCapabilities)
        .timeout(DEADLINE)
        .await
        .map_err(|_| anyhow!("native typed capabilities read failed"))?;
    validate_capabilities(&capabilities, &installation.device)?;
    report["capabilitiesStartupSnapshot"] = serde_json::to_value(capabilities)?;
    record(
        report,
        "resources/read embedding://capabilities",
        started,
        json!({"validated":true,"freshRuntimeHealthClaim":false}),
    );

    let started = Instant::now();
    let before = installation.usage().await?;
    record(
        report,
        "GET own usage/daily before",
        started,
        before.clone(),
    );
    let input = corpus();
    let started = Instant::now();
    let output = client
        .call(input.clone())
        .timeout(DEADLINE)
        .await
        .map_err(|_| anyhow!("real embedding tool failed; no automatic replay"))?;
    record(
        report,
        "tools/call embedding__embed",
        started,
        json!({"requestId":output.request_id}),
    );
    validate_result(
        &output,
        &RuntimeRequest {
            request_id: output.request_id,
            items: input.items,
        },
        &installation.device,
    )?;
    let scores = semantic_scores(&output)?;
    let bytes = serde_json::to_vec(&output)?;
    report["outputEvidence"] = json!({"encoding":"serde_json compact reserialization of decoded structured tool output; not HTTP wire bytes", "bytes":bytes.len(),"sha256":digest(&bytes),"maxNormError":output.items.iter().map(|item| (item.vector.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt()-1.0).abs()).fold(0.0_f64,f64::max),"semanticScores":scores});
    report["output"] = serde_json::to_value(&output)?;
    let started = Instant::now();
    let after = installation.usage().await?;
    record(
        report,
        "GET own usage/daily after success",
        started,
        after.clone(),
    );
    usage_delta(&before, &after, output.usage.input_tokens, MAX_BATCH_TOKENS)?;

    let overflow = EmbedRequest {
        items: vec![EmbeddingInput::SearchQuery {
            text: " x".repeat(16384),
        }],
    };
    overflow
        .validate()
        .map_err(|_| anyhow!("overflow fixture violates byte admission"))?;
    report["rejectionInput"] = json!({"purpose":"search_query","textRecipe":{"unit":" x","repeat":16384},"requestJsonSha256":digest(&serde_json::to_vec(&overflow)?)});
    let started = Instant::now();
    match client.call(overflow).timeout(DEADLINE).await {
        Err(CallError::Rejected {
            code: -32602,
            message,
        }) if message.contains("item_token_limit")
            && message.contains("inference not started")
            && message.contains("zero actual usage recorded") => {}
        _ => bail!("overflow did not produce the exact native pre-execution input rejection"),
    }
    record(
        report,
        "tools/call embedding__embed overflow",
        started,
        json!({"code":"item_token_limit","execution":"not_started","observedGpuDispatchCounter":null}),
    );
    let started = Instant::now();
    let rejected = installation.usage().await?;
    record(
        report,
        "GET own usage/daily after rejection",
        started,
        rejected.clone(),
    );
    usage_delta(&after, &rejected, 0, MAX_ITEM_TOKENS)?;

    // Same valid modern unauthenticated envelope used by existing native acceptance.
    let started = Instant::now();
    let response = installation.http.post(client.endpoint().clone())
        .header(reqwest::header::CONTENT_TYPE,"application/json")
        .header(reqwest::header::ACCEPT,"application/json, text/event-stream")
        .header("MCP-Protocol-Version","2026-07-28").header("Mcp-Method","tools/list")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}))
        .send().await.map_err(|_| anyhow!("unauthenticated native probe failed in transport"))?;
    ensure!(
        response.status() == reqwest::StatusCode::UNAUTHORIZED
            && response
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .is_some_and(|v| v.to_str().is_ok_and(|v| v.starts_with("Bearer"))),
        "missing native 401 Bearer challenge"
    );
    record(
        report,
        "unauthenticated tools/list",
        started,
        json!({"httpStatus":401,"bearerChallenge":true}),
    );

    if let Some(token) = &installation.restricted {
        let restricted = installation.client(token)?;
        let started = Instant::now();
        let tools = restricted
            .tools()
            .await
            .map_err(|_| anyhow!("restricted identity discovery failed"))?;
        ensure!(
            !tools
                .items
                .iter()
                .any(|tool| tool.name == "embedding__embed"),
            "restricted identity unexpectedly advertises compute"
        );
        match restricted.call(corpus()).timeout(DEADLINE).await {
            Err(CallError::Rejected { message, .. })
                if message.contains("embedding.embeddings.create") => {}
            _ => bail!("restricted identity did not produce a native compute-permission refusal"),
        }
        record(
            report,
            "restricted discovery and direct compute refusal",
            started,
            json!({"status":"passed","noGrantChanges":true}),
        );
    } else {
        report["insufficientPermissionCase"] = json!({"status":"unavailable","reason":"no existing read-only bearer supplied; current member role includes write; no grants were added"});
    }
    if let Some(path) = &installation.baseline {
        ensure!(
            std::fs::metadata(path)?.len() <= MAX_EVIDENCE_BYTES as u64,
            "baseline report exceeds byte bound"
        );
        let baseline: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            baseline["status"] == "passed" && baseline["inputs"] == report["inputs"],
            "baseline status/corpus differs"
        );
        let reference: EmbeddingResult = serde_json::from_value(baseline["output"].clone())?;
        validate_result(
            &reference,
            &RuntimeRequest {
                request_id: reference.request_id,
                items: corpus().items,
            },
            &reference.execution.device,
        )?;
        report["crossDeviceParity"] = parity(&reference, &output)?;
    } else {
        report["crossDeviceParity"] = json!({"status":"not_requested","reason":"supply prior native Hub acceptance report on the second device"});
    }
    Ok(())
}

async fn run_capacity_case(
    installation: &Installation,
    client: &HubClient,
    capabilities: &Capabilities,
    case: &CapacityCase,
    report: &mut Value,
) -> Result<()> {
    case.input
        .validate()
        .map_err(|_| anyhow!("capacity recipe violates byte admission"))?;
    let started = Instant::now();
    let before = installation.usage().await?;
    record(
        report,
        "GET own usage/daily before",
        started,
        before.clone(),
    );
    let started = Instant::now();
    let output = client
        .call(case.input.clone())
        .timeout(DEADLINE)
        .await
        .map_err(|_| anyhow!("real capacity embedding tool failed; no automatic replay"))?;
    record(
        report,
        "tools/call embedding__embed",
        started,
        json!({"requestId":output.request_id}),
    );
    validate_result(
        &output,
        &RuntimeRequest {
            request_id: output.request_id,
            items: case.input.items.clone(),
        },
        &installation.device,
    )?;
    ensure!(
        output.model == capabilities.model
            && output.processing_id == capabilities.processing_id
            && output.execution == capabilities.execution
            && output.runtime == capabilities.runtime,
        "capacity output identity differs from validated startup snapshot"
    );
    ensure!(
        output
            .items
            .iter()
            .map(|item| item.token_count)
            .eq(case.expected_tokens.iter().copied())
            && output.usage.input_tokens == case.expected_tokens.iter().sum::<u64>(),
        "capacity output differs from exact expected per-item/total token counts"
    );
    let bytes = serde_json::to_vec(&output)?;
    report["outputEvidence"] = json!({
        "encoding":"serde_json compact reserialization of decoded structured tool output; not HTTP wire bytes",
        "bytes":bytes.len(),"sha256":digest(&bytes),
        "maxNormError":output.items.iter().map(|item| (item.vector.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt()-1.0).abs()).fold(0.0_f64,f64::max),
        "orderedItems":output.items.iter().map(|item| json!({"index":item.index,
            "purpose":item.purpose,"dimensions":item.vector.len(),"tokenCount":item.token_count})).collect::<Vec<_>>()});
    report["output"] = serde_json::to_value(&output)?;
    let started = Instant::now();
    let after = installation.usage().await?;
    record(
        report,
        "GET own usage/daily after success",
        started,
        after.clone(),
    );
    usage_delta(&before, &after, output.usage.input_tokens, MAX_BATCH_TOKENS)?;
    report["validatedUsageDelta"] = json!({"estimate":{"tokens":MAX_BATCH_TOKENS,"records":1},
        "actual":{"tokens":output.usage.input_tokens,"records":1}});
    Ok(())
}

async fn run_capacity(installation: &Installation, report: &mut Value) -> Result<()> {
    let client = installation.client(&installation.member)?;
    let started = Instant::now();
    let tools = client
        .tools()
        .await
        .map_err(|_| anyhow!("native Hub capacity discovery failed"))?;
    record(
        report,
        "tools/list",
        started,
        json!({"embeddingAdvertised":tools.items.iter().any(|tool| tool.name == "embedding__embed"),
            "catalogComplete":tools.is_complete(),"toolCount":tools.items.len(),
            "toolNames":tools.items.iter().take(256).map(|tool| tool.name.as_ref()).collect::<Vec<_>>(),
            "toolNamesTruncated":tools.items.len()>256,
            "degradation":tools.degradation.failures.iter().take(256).map(|failure|
                json!({"server":failure.server,"surface":failure.surface,"code":failure.code})).collect::<Vec<_>>(),
            "degradationTruncated":tools.degradation.failures.len()>256}),
    );
    ensure!(
        tools
            .items
            .iter()
            .any(|tool| tool.name == "embedding__embed"),
        "embedding tool absent from authenticated capacity discovery"
    );
    let started = Instant::now();
    let capabilities = client
        .read(ReadCapabilities)
        .timeout(DEADLINE)
        .await
        .map_err(|_| anyhow!("native typed capacity capabilities read failed"))?;
    validate_capabilities(&capabilities, &installation.device)?;
    report["capabilitiesStartupSnapshot"] = serde_json::to_value(&capabilities)?;
    record(
        report,
        "resources/read embedding://capabilities",
        started,
        json!({"validated":true,"freshRuntimeHealthClaim":false}),
    );
    for case in capacity_cases() {
        let input_bytes = serde_json::to_vec(&case.input)?;
        let mut evidence = json!({"case":case.name,"inputRecipe":case.recipe,
            "inputJson":{"encoding":"compact native EmbedRequest JSON without runtime-assigned request ID",
                "bytes":input_bytes.len(),"sha256":digest(&input_bytes)},
            "expectedTokenCounts":case.expected_tokens,"sequence":[],"status":"in_progress"});
        let started = Instant::now();
        let outcome =
            run_capacity_case(installation, &client, &capabilities, &case, &mut evidence).await;
        evidence["elapsedMs"] = json!(elapsed(started));
        evidence["status"] = json!(if outcome.is_ok() { "passed" } else { "failed" });
        if let Err(error) = &outcome {
            evidence["failure"] = json!(error.to_string());
        }
        report["cases"]
            .as_array_mut()
            .expect("capacity cases")
            .push(evidence);
        outcome?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicitly configured native Hub and real pinned GPU runtime; two capacity calls"]
async fn native_hub_real_embedding_capacity_and_usage() -> Result<()> {
    let installation = Installation::load()?;
    ensure!(
        !installation.report.exists(),
        "choose a new report path; preserve earlier receipts"
    );
    let mut report = json!({"scenario":"native_hub_real_embedding_token_capacity",
        "startedAt":chrono::Utc::now(),"runnerSourceSha256":digest(include_bytes!("acceptance.rs")),
        "agentModel":"none; deterministic Rust native client","hubOrigin":installation.origin,
        "profile":installation.profile,
        "expectedCaller":{"tenant":installation.tenant,"principal":installation.principal,"workContext":installation.context},
        "sequence":[],"cases":[],"status":"in_progress",
        "proofLimits":["Real capacity calls use the exact public runtime token-ceiling input recipes through the native Hub SDK; tokenizer totals are checked against the pinned-model expectations.",
            "Ordered indices/purposes, complete output, finite 768-dimensional unit vectors and exact per-item tokens are validated; no separate per-item execution trace is claimed.",
            "Native own-usage exposes authenticated caller tenant and principal/kind/phase/token totals, not request-level context/delegation or declaration metadata.",
            "Exact per-case deltas require other embedding activity for this principal paused; they do not independently prove exclusive attribution.",
            "Output bytes/hashes cover compact structured-output reserialization, not HTTP wire bytes.",
            "Busy and disconnect handling are not exercised here; those remain separately recorded Mac/common-code qualification."]});
    let started = Instant::now();
    let outcome = run_capacity(&installation, &mut report).await;
    report["elapsedMs"] = json!(elapsed(started));
    report["status"] = json!(if outcome.is_ok() { "passed" } else { "failed" });
    if let Err(error) = &outcome {
        report["failure"] = json!(error.to_string());
    }
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(
        bytes.len() <= MAX_EVIDENCE_BYTES,
        "report exceeds byte bound"
    );
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&installation.report)?;
    file.write_all(&bytes)?;
    eprintln!(
        "Embedding capacity report: {} ({} bytes, {})",
        installation.report.display(),
        bytes.len(),
        digest(&bytes)
    );
    outcome
}

#[tokio::test]
#[ignore = "requires explicitly configured native Hub and real pinned GPU runtime"]
async fn native_hub_real_embedding_corpus_and_usage() -> Result<()> {
    let installation = Installation::load()?;
    ensure!(
        !installation.report.exists(),
        "choose a new report path; preserve earlier receipts"
    );
    let mut report = json!({"scenario":"native_hub_real_embedding_six_item","startedAt":chrono::Utc::now(),
        "runnerSourceSha256":digest(include_bytes!("acceptance.rs")),"agentModel":"none; deterministic Rust native client",
        "hubOrigin":installation.origin,"profile":installation.profile,
        "expectedCaller":{"tenant":installation.tenant,"principal":installation.principal,"workContext":installation.context},
        "inputs":corpus(),"sequence":[],"status":"in_progress",
        "proofLimits":["Native own-usage API scopes to the authenticated caller tenant and exposes principal/kind/phase/token totals, not individual request attribution.","Expected tenant/context are trusted fixture inputs; live work-context/delegation and kind-declaration metadata are not exposed by this daily API. Native component tests cover draft attribution/declaration.","Exact bounded deltas require other embedding activity for this same principal paused; they do not independently prove exclusive attribution.","Not-started is the runtime's explicit typed assertion plus zero actual ledger usage, not an independent GPU dispatch counter.","Output byte/hash evidence is compact structured-output reserialization, not captured HTTP wire bytes."]});
    let started = Instant::now();
    let outcome = run(&installation, &mut report).await;
    report["elapsedMs"] = json!(elapsed(started));
    report["status"] = json!(if outcome.is_ok() { "passed" } else { "failed" });
    if let Err(error) = &outcome {
        report["failure"] = json!(error.to_string());
    }
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(
        bytes.len() <= MAX_EVIDENCE_BYTES,
        "report exceeds byte bound"
    );
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&installation.report)?;
    file.write_all(&bytes)?;
    eprintln!(
        "Embedding acceptance report: {} ({} bytes, {})",
        installation.report.display(),
        bytes.len(),
        digest(&bytes)
    );
    outcome
}

#[test]
fn capacity_recipes_preserve_public_input_bytes_and_exact_token_expectations() -> Result<()> {
    let cases = capacity_cases();
    for case in &cases {
        case.input
            .validate()
            .map_err(|_| anyhow!("invalid capacity input"))?;
        ensure!(
            case.input.items.len() == case.expected_tokens.len(),
            "token count length differs"
        );
        for (index, item) in case.input.items.iter().enumerate() {
            let EmbeddingInput::Document { content, title } = item else {
                bail!("capacity recipe purpose differs");
            };
            if case.expected_tokens[index] == MAX_ITEM_TOKENS {
                ensure!(
                    content.len() == 16360
                        && digest(content.as_bytes())
                            == "sha256:20c36711eccc025c3c2e6f2b4ea898ef38e675546a927a41f647bd9f4ea9bb62",
                    "long document bytes differ from published runtime fixture"
                );
                let expected_title = if index == 0 {
                    "Token ceiling document A"
                } else {
                    "Token ceiling document B"
                };
                ensure!(
                    title.as_deref() == Some(expected_title),
                    "long document title/order differs"
                );
            } else {
                ensure!(
                    case.expected_tokens[index] == 15
                        && content == &format!("short input item {index}")
                        && title.as_deref() == Some(format!("short-{index}").as_str()),
                    "short document bytes/order differ from published runtime fixture"
                );
            }
        }
    }
    ensure!(
        cases[0].expected_tokens == [8192, 8192]
            && cases[0].expected_tokens.iter().sum::<u64>() == MAX_BATCH_TOKENS
            && cases[1].expected_tokens == [8192, 15, 15, 15, 15, 15, 15, 15]
            && cases[1].expected_tokens.iter().sum::<u64>() == 8297,
        "capacity token expectations differ"
    );
    // This verifies the public recipe, not tokenizer/model execution. Only the
    // explicitly ignored native test can establish the real token counts.
    Ok(())
}

#[test]
fn fixed_corpus_and_native_decimal_usage_delta_are_stable() -> Result<()> {
    ensure!(
        corpus().items.len() == 6 && corpus().validate().is_ok(),
        "invalid fixed corpus"
    );
    let page = |actual: &str, estimate: &str, records: u64| {
        json!({"rows":[
        {"phase":"actual","quantity":actual,"records":records},
        {"phase":"estimate","quantity":estimate,"records":records}]})
    };
    usage_delta(
        &page("12", "8192", 1),
        &page("12.000000000000000000", "16384", 2),
        0,
        8192,
    )?;
    ensure!(
        usage_delta(&page("12", "8192", 1), &page("13", "16384", 2), 0, 8192).is_err(),
        "accepted wrong zero-actual delta"
    );
    Ok(())
}

#[test]
fn parity_checks_exact_semantic_space_order_and_declared_numeric_thresholds() -> Result<()> {
    // Synthetic unit vectors test comparison only; never model inference evidence.
    let items: Vec<_> = corpus()
        .items
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let mut vector = vec![0_f32; DIMENSIONS];
            vector[[0, 0, 1, 2, 2, 3][index]] = 1.0;
            json!({"index":index,"purpose":input.purpose(),"vector":vector,"tokenCount":1})
        })
        .collect();
    let reference: EmbeddingResult = serde_json::from_value(json!({
        "requestId":uuid::Uuid::now_v7(),"model":{"id":MODEL_ID,"revision":MODEL_REVISION},
        "processingId":PROCESSING_ID,"dimensions":DIMENSIONS,"normalized":true,
        "execution":{"device":"mps","dtype":"float32"},
        "runtime":{"package":"veoveo-runtime-embedding","version":"0.1.0","sourceRevision":"synthetic","python":"3.14.6","sentenceTransformers":"6.1.0","transformers":"5.20.2","torch":"2.14.1","torchvision":"0.29.1","pillow":"13.0.0"},
        "items":items,"usage":{"inputTokens":6}
    }))?;
    parity(&reference, &reference)?;
    let mut changed = reference.clone();
    changed.items[0].vector[7] = 0.002;
    ensure!(
        parity(&reference, &changed).is_err(),
        "accepted excess absolute difference"
    );
    changed = reference.clone();
    changed.items.swap(1, 2);
    ensure!(
        parity(&reference, &changed).is_err(),
        "accepted changed order"
    );
    changed = reference.clone();
    changed.processing_id = "wrong-space".into();
    ensure!(
        parity(&reference, &changed).is_err(),
        "accepted different semantic space"
    );
    Ok(())
}
