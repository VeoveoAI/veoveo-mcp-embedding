# Deployed qualification inputs

These ignored Rust tests call the real Hub and model only when explicitly selected.
Ordinary component tests use synthetic vectors. The operator prepares Artifact objects,
credentials and network policy independently; the runner creates no users, groups or ACL
grants and changes no production server API. Keep reports outside this repository.

Set `VEOVEO_EMBEDDING_ACCEPTANCE=1` and these `VEOVEO_ACCEPTANCE_` variables:
`HUB` (verified HTTPS origin), `PROFILE` (default local), `DEVICE` (mps/cuda),
`MEMBER_BEARER_ENV` (name of an existing environment variable holding the bearer),
`PRINCIPAL`, `TENANT`, `CASES_FILE`, and `REPORT` (new path in an existing
directory). Optional `CA_FILE` supplies the admitted CA bundle; `BASELINE_REPORT` selects
a previously passed corpus/capacity report; `READ_ONLY_BEARER_ENV` selects a pre-existing
identity denied embedding permission. Bearers and signed URLs never enter reports.

Use the exact reviewed Git lock before deployment. Select one ignored test at a time:

```sh
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_corpus_and_usage -- --ignored --exact
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_capacity_and_usage -- --ignored --exact
cargo test --locked -p veoveo-mcp-embedding --test acceptance native_hub_real_embedding_rejections -- --ignored --exact
```

## Success manifest

All fields are closed. `kind` is corpus or capacity; at most 16 serial cases run. A
media part of a manifest's input names its fixture Artifact; the runner signs it right
before the case's call, so a manifest holds no signed URL. This illustrative manifest
requires real UUIDs and measured fixture values:

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

Before any live call, the runner validates public inputs, each with its fixtures' stated
MIME/size in its parts' places, and any baseline, including the baseline's independent
fixture-recipe fingerprints. Artifact IDs may differ across installations; fixtures are
compared by position. Before inference, the test caller signs each fixture's download
through `artifact__create_download` and streams it within its exact-size bound,
computing its own SHA-256 against the supplied recipe. This test-only evidence is not an
Artifact metadata field or a production MCP byte-fetch path. It then calls typed `embed`
with the signed media, without retry. Signed URLs/headers stay private: a receipt keeps
the case input with its Artifact IDs and the request shape, never a signed URL. Each
validated output, measured embed time and compact JSON byte/hash receipt is stored
before ranking, accounting or parity assertions. Failed later checks
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
  }]
}
```

The retained invalid-PNG-byte probe measured `preprocessing_failed` from PIL through
the native backend; this example uses that code. Other malformed codecs/recipes can
produce different closed codes and require their own measured expectations.
A runtime rejection must match the service's closed request-bound not-started response;
one estimate and one zero-actual usage record must appear. Unknown SDK/private messages
are never copied into reports. Embedding authorizes no media: Artifact decides access
when the caller signs a download, so ACL denial is Artifact's to qualify, not this
runner's.

## Temporary native composed caller

`native_hub_registered_composition_and_usage` registers the test-only `embeddingprobe`
server with one typed `forward_embed` tool and declared embedding-create use. It creates
no Tasks and adds no production route. The single corpus case calls caller → Embedding
through native SDK ServerCalls, with media the test caller signed; use a joint media case
to carry signed media through the composed hop.

Root/builder must first admit the exact logical HTTP endpoint, fixed bind address, startup
native Hub binding and existing registration credential custody. A ServiceExposure alone
does not install a Hub binding, and no separate ingress exposure is needed. Set `VEOVEO_ACCEPTANCE_COMPOSED_PROBE=1`, plus prefixed variables
`PROBE_DEPLOYMENT_ID` (admitted UUIDv7), `PROBE_PUBLIC_BASE_URL`, `PROBE_BIND` (explicit nonzero socket port),
`PROBE_ALLOWED_HOSTS` (comma-separated admitted authorities), `REGISTRATION_SECRET_ENV`
(existing environment selector). `PROBE_ALLOW_LOOPBACK=1` is explicit and only for the
operator's admitted deployment. The runner changes no network policy. Native readiness
uses the existing member through native HubClient tools/list and read_resource at
embeddingprobe://contract. It requires embeddingprobe__forward_embed and the exact native
hosted-MCP declaration for server embeddingprobe. Metadata reads can poll within the
ten-second phase; the inference runner is invoked once. Readiness counts/timing and the
validated declaration are preserved in compositionReadiness. There is no direct /livez
request or alternate loopback/proxy route. Its native ephemeral signing key is only
for this registration, not a replacement registration credential.

Discovery/registration/store/instance/readiness/shutdown phases are bounded by ten
seconds each; composed work by 120 seconds, individual calls by 65 seconds. Native
explicit RFC 7592 retirement deregisters after work/cleanup; its HTTP request has the native ten-second SDK timeout. The listener
is aborted on drop, normal shutdown waits at most ten seconds, then the process instance
and store are closed. Abrupt process termination or failed deregistration requires the
operator to inspect native registration/instance state; the receipt reports failure.
Do not replay an ambiguous inference to repair a fixture lifecycle failure.

## Python producer/Rust consumer smoke

No model call or network is needed to validate a retained safe Python fixture with exact
keys `capabilities`, `input`, `output`. `input` is the request shape: the request with
each media part's `url` and `headers` left out, `{"type":"image","mediaType":...,"size":...}`,
so the fixture holds no signed URL:

```sh
VEOVEO_EMBEDDING_WIRE_FIXTURE=/absolute/retained-wire.json cargo test --locked -p veoveo-mcp-embedding --test validation actual_python_wire_is_accepted_by_rust_without_another_model_call -- --ignored --exact
```

The fixtures must contain actual serialized Python output and initialized runtime
capabilities; synthetic fixtures are labelled separately. This checks wire/validation
compatibility and does not add independent inference evidence.
