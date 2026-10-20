# Deployed qualification inputs

These ignored Rust tests call the real Hub and model only when explicitly selected.
Ordinary component tests use synthetic vectors. The operator prepares Artifact objects,
credentials and network policy independently; the runner creates no users, Work Contexts
or ACL grants and changes no production server API. Keep reports outside this repository.

Set `VEOVEO_EMBEDDING_ACCEPTANCE=1` and these `VEOVEO_ACCEPTANCE_` variables:
`HUB` (verified HTTPS origin), `PROFILE` (default local), `DEVICE` (mps/cuda),
`MEMBER_BEARER_ENV` (name of an existing environment variable holding the bearer),
`PRINCIPAL`, `TENANT`, `WORK_CONTEXT`, `CASES_FILE`, and `REPORT` (new path in an existing
directory). Optional `CA_FILE` supplies the admitted CA bundle; `BASELINE_REPORT` selects
a previously passed corpus/capacity report; `READ_ONLY_BEARER_ENV` selects a pre-existing
identity denied embedding permission. Bearers and signed transfers never enter reports.

Use the exact reviewed Git lock before deployment. Select one ignored test at a time:

```sh
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_corpus_and_usage -- --ignored --exact
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_capacity_and_usage -- --ignored --exact
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_rejections_and_artifact_denial -- --ignored --exact
```

## Success manifest

All fields are closed. `kind` is corpus or capacity; at most 16 serial cases run.
This illustrative manifest requires real UUIDs and measured fixture values:

```json
{
  "kind": "corpus",
  "cases": [{
    "name": "ordered-two-items",
    "input": {
      "items": [
        {"content": [{"type": "text", "text": "hello again"}]},
        {"content": [
          {"type": "text", "text": "compare these scenes"},
          {"type": "image", "artifactId": "<image-one-id>"},
          {"type": "image", "artifactId": "<image-two-id>"}
        ]}
      ],
      "dimensions": 256
    },
    "fixtures": [
      {"artifactId": "<image-one-id>", "mediaType": "image/png", "size": 123,
       "sha256": "sha256:<64-lowercase-hex>", "recipe": "exact deterministic fixture recipe"},
      {"artifactId": "<image-two-id>", "mediaType": "image/png", "size": 456,
       "sha256": "sha256:<64-lowercase-hex>", "recipe": "second exact recipe"}
    ],
    "expectedTokens": [12, 345],
    "rankings": []
  }]
}
```

`expectedTokens` is optional except capacity cases, which must include a previously
measured 8192-token item (use the retained exact text/media/processing recipe, not a
character-count approximation). `rankings` entries contain zero-based `query`,
`preferred`, `other` item indices; their cosine ordering is an explicit corpus hypothesis,
not a model guarantee. Exercise independent text/media items, joint ordered modalities,
repeated images/audio/video parts, dimensions and optional native soundtrack separately.
Soundtrack inclusion cannot coexist with separate audio parts in the batch.

Before any live call, the runner validates public inputs and any baseline, including
per-part MIME/size/SHA256 against the new recipes. Artifact IDs may differ across
installations. It then reads safe native Artifact metadata and calls typed `embed`
without retry. Each validated output, measured embed time and compact JSON byte/hash
receipt is stored before ranking, accounting or parity assertions. Failed later checks
retain that output. Byte evidence describes structured-result reserialization, not raw
HTTP bytes. Daily own usage confirms selected-principal totals, not full delegation
attribution. Cross-device parity requires equal processing/source bytes and checks
cosine ≥ 0.9999 and maximum component difference ≤ 0.0001.

## Negative manifest

Use `kind: "negative"` with at most eight cases. A malformed image case uses an Available
Artifact whose declared MIME is image/png but whose known bytes are deliberately invalid;
include its exact safe fixture recipe. The public input is otherwise valid:

```json
{
  "kind": "negative",
  "cases": [{
    "name": "malformed-image",
    "input": {"items": [{"content": [{"type": "image", "artifactId": "<invalid-image-id>"}]}]},
    "fixtures": [{"artifactId": "<invalid-image-id>", "mediaType": "image/png", "size": 7,
      "sha256": "sha256:<64-lowercase-hex>", "recipe": "exact seven invalid image bytes"}],
    "expect": {"type": "runtime_not_started", "code": "preprocessing_failed"}
  }, {
    "name": "actual-artifact-acl-denial",
    "input": {"items": [{"content": [{"type": "image", "artifactId": "<existing-restricted-id>"}]}]},
    "expect": {"type": "artifact_denied", "prepared": null}
  }]
}
```

The retained invalid-PNG-byte probe measured `preprocessing_failed` from PIL through
the native backend; this example uses that code. Other malformed codecs/recipes can
produce different closed codes and require their own measured expectations.
A runtime rejection must match the service's closed request-bound not-started response;
one estimate and one zero-actual usage record must appear. Unknown SDK/private messages
are never copied into reports. For ACL denial, `prepared: null` explicitly records
`not_exercised` and makes no call for that case. To exercise it, an owner/preparer must
provide a known existing Artifact outside the selected caller's actual read ACL:

```json
{"artifactId": "<existing-restricted-id>", "owner": "<owner-principal-id>",
 "workContext": "<artifact-context-id>", "preparer": "<preparer-principal-id>",
 "evidenceSha256": "sha256:<owner-preparation-receipt-hash>",
 "callerHasNoReadAccess": true}
```

Different ownership in the same Work Context
is insufficient because membership grants access. A random missing ID or unavailable
metadata does not prove ACL denial. The runner checks native Artifact read refusal and
closed embedding resolution refusal, then requires unchanged own usage (no reservation).
External preparation evidence proves existence/ACL setup; these observed refusals alone
do not reveal Artifact's internal SQL decision.

## Temporary native composed caller

`native_hub_registered_composition_and_usage` registers the test-only `embeddingprobe`
server with one typed `forward_embed` tool and declared embedding-create use. It creates
no Tasks and adds no production route. The single corpus case calls caller → Embedding
→ Artifact through native SDK ServerCalls; use a joint media case to exercise both hops.

Root/builder must first admit the public endpoint, bind address and existing registration
credential custody. Set `VEOVEO_ACCEPTANCE_COMPOSED_PROBE=1`, plus prefixed variables
`PROBE_PUBLIC_BASE_URL`, `PROBE_BIND` (explicit nonzero socket port),
`PROBE_ALLOWED_HOSTS` (comma-separated admitted authorities), `REGISTRATION_SECRET_ENV`
(existing environment selector). `PROBE_ALLOW_LOOPBACK=1` is explicit and only for the
operator's admitted deployment. The runner changes no network policy and uses no
alternate loopback/proxy route for readiness. Its native ephemeral signing key is only
for this registration, not a replacement registration credential.

Discovery/registration/store/instance/readiness/shutdown phases are bounded by ten
seconds each; composed work by 120 seconds, individual calls by 65 seconds. Native
maintain deregisters after work/cleanup; its HTTP request has the native ten-second SDK timeout. The listener
is aborted on drop, normal shutdown waits at most ten seconds, then the process instance
and store are closed. Abrupt process termination or failed deregistration requires the
operator to inspect native registration/instance state; the receipt reports failure.
Do not replay an ambiguous inference to repair a fixture lifecycle failure.

## Python producer/Rust consumer smoke

No model call or network is needed to validate a retained safe Python fixture with exact
keys `capabilities`, `input`, `sources`, `output`. `sources` is one array per item of
`{partIndex,source:{artifactId,mediaType,size,sha256}}`, with no transfer grants:

```sh
VEOVEO_EMBEDDING_WIRE_FIXTURE=/absolute/retained-wire.json cargo test --locked -p veoveo-mcp-embedding --test validation actual_python_wire_is_accepted_by_rust_without_another_model_call -- --ignored --exact
```

The fixtures must contain actual serialized Python output and initialized runtime
capabilities; synthetic fixtures are labelled separately. This checks wire/validation
compatibility and does not add independent inference evidence.
