# Multimodal embedding generation

This service owns typed MCP discovery, native caller-bound Artifact authorization and
Core usage. The private runtime owns bounded media transfer/decoding and the full
pinned GPU model. Historical purpose-based text-only deployment evidence does not
qualify this replacement. No vector storage, search, Knowledge integration, custom
sampler, clipping/chunking/aggregation, model choice or compatibility route is added.

## Public ordered API

`embed` requires `embedding.embeddings.create`; capabilities/docs require
`embedding.models.read`. Ready SDK routes own discovery, authorization and dispatch.
The contract-only feature contains no network, database or model runtime dependencies;
media IDs reuse Artifact's contract type.

```json
{
  "items": [
    {"content": [{"type": "text", "text": "hello again"}]},
    {"content": [
      {"type": "text", "text": "A product demonstration"},
      {"type": "video", "artifactId": "<available-video-artifact-id>"}
    ]}
  ],
  "dimensions": 256
}
```

For the two-item request above, the result contains two rows (the ellipses below
are illustrative and are not valid JSON):

```text
{
  "dimensions": 256,
  "items": [
    {"index": 0, "vector": [0.12, -0.03, ...], "tokenCount": ..., "media": []},
    {"index": 1, "vector": [0.08, 0.04, ...], "tokenCount": ..., "media": [...]}
  ],
  ...
}
```

N ordered items produce N normalized vectors. All ordered parts of one item produce
one joint vector. An item can contain repeated modalities. Code uses ordinary text.
Dimensions default 768, with 128/256/512 supported through upstream Matryoshka truncation
and normalization. Optional request-level `promptName` or literal `prompt` applies once
to the complete item. They are exclusive; omission means no prefix. Names are
SearchQuery, CodeRetrieval, Document, QuestionAnswering, FactChecking, Classification,
Clustering and SentenceSimilarity. These choose native prefixes in the same model;
they do not select different models or embedding spaces. Caller text/prompt cannot
contain reserved media placeholders, which the upstream template interprets specially.

Closed `preprocessing` fields mirror audited native operations:

| Group | Fields and defaults |
|---|---|
| image | maxSoftTokens 280; choices 70/140/280/560/1120 |
| audio | truncation false; maxLength samples only if true (native default 480000) |
| video | fps 1, maxFrames 32, overflowStrategy uniform (or truncate), maxSoftTokens 140, addTimestamps false, loadAudioFromVideo false |

Audio uses native 16 kHz decoding/resampling without silent 30-second truncation. Native video
soundtrack inclusion is refused alongside separate audio parts because of the pinned
processor interaction. Missing requested soundtrack fails explicitly. These are upstream
CPU preprocessing operations, distinct from FP32 MPS/CUDA inference. No GPU decoding
claim is made. Sampling/truncation coverage is returned; application clip extraction,
long-media chunking or aggregation would be separate proposed pre/postprocessing.

Initial Veoveo service bounds are 8 items, 16 parts/item, 32 parts/batch, 64 KiB total UTF-8
text/prompt, 64 MiB/source and 128 MiB aggregate transfer; 16 MP/image or frame and 64 MP
aggregate selected pixels; 300 seconds/source and 600 seconds aggregate decoded audio; video fps ≤ 120
and maxFrames ≤ 256. These defensive limits require deployed capacity qualification,
and are not Google capability or semantic-quality guarantees. Actual native tokens
must fit 8192/item and 16384/batch; all preflight precedes the first model forward.

## Artifact and private runtime boundary

The service declares Artifact read use. Native ServerCalls are configured from the
same registration key/metadata/HTTP client; handlers use `ctx.client()` and typed
CreateDownloadRequest through the Hub. Artifact owns ACL and immutable availability.
MCP requires the exact requested ID, Available state, known positive bounded size and
SHA256, the matching MIME family (audio also accepts video containers), and a live GET.
It checks aggregate sizes before any runtime dispatch. No bulk bytes pass through MCP.

The private server-only DTO replaces Artifact references with their authorized
identity/mediaType/size/digest plus Artifact's actual typed SignedTransfer. Empty
headers may be omitted. URLs/headers never enter public schemas, results, errors,
Debug output or logs. Private serialized payload also must fit 512 KiB; public text
bounds alone do not constrain signed-header expansion. Runtime independently enforces
its configured storage origin, no redirects/proxies, elapsed deadline, raw byte/size/hash
and native decoded bounds. Verified temporary files remain owned by its actual worker.
Metadata validation and repeated bounded native decodes are explicit runtime costs.

All references resolve before inference usage reservation. Text-only requests require
no Artifact call. One 60-second deadline covers resolution and generation; failed/denied or
timed-out resolution reports not_started with no inference usage reservation. Once
generation begins, timeout conservatively reports execution/accounting may be incomplete.
One runtime GPU slot remains owned until actual work finishes, even after disconnect;
there is no retry, queue, kernel-cancellation guarantee or durable embedding Task.

The fixed runtime endpoint accepts a private bearer resolved through admitted native
OpenShell policy. Redirects/retries are disabled; idle connections retire after 4 seconds,
before the runtime's 5-second keepalive. Capability identity is validated at startup; the
public capability resource is that snapshot, not a fresh-health claim. /readyz rechecks
within 3 seconds. No request downloads a model. Installation owns policies/cache/prefetch.

## Output validation and provenance

Every result must match fresh UUIDv7 request ID, ordered indices, item/media cardinality,
requested dimensions, finite unit vectors, actual per-item/aggregate token bounds and
nonmonetary usage totals. Model/checkpoint, FP32 device and all pinned package versions
are checked, including exact MPS vs CUDA wheel suffixes and TorchCodec. FFmpeg versions
must be structurally valid native provenance. Runtime source revision is a commit ID.

ProcessingId hashes RFC8785/JCS bytes of the effective model/dimensions/prompt/native
processing/layout/pooling/audio-rate descriptor. Rust uses exact
serde_json_canonicalizer 0.3.2, already shared by Core; Python uses rfc8785==0.1.4. This
shared pin is deliberate (upstream also has 0.4.1). Frozen Python-generated Unicode and
numeric cases prove canonical byte/hash parity; ordinary sorted JSON is insufficient.
Input content is excluded except explicit effective prompt; custom prompt is not echoed,
only its digest and optional canonical name.

Closed image/audio/video coverage must correspond to each exact ordered media part and
authorized safe source identity. Pixel/duration/frame/sample bounds, native option effects,
truncation/retained samples and soundtrack presence are checked. Native repeated frame
indices are valid: order is nondecreasing, not unique. Model prompt timestamps equal
index/sourceFPS; actual codec PTS are separate and can have source timeline offsets.
Validation does not reimplement Google's sampler or independently infer semantics from
pixels. Typed public result validation can use safe source identities for saved receipts,
without constructing fake grants or retaining signed URLs.

## Accounting and native composition

Core UsageStore/UsageWrite support synchronous task=None usage. Reserve
min(N × 8192, 16384) as an estimate before runtime dispatch, never as measured tokens.
Validated success supplies actual tokens. A closed correctly status/request-bound
not_started runtime rejection records zero actual. Unknown bodies/codes, disconnect,
timeout or malformed output retain estimate as ambiguous. Accounting failure after
successful inference explicitly says inference completed; never replay it.

Caller tenant/principal/Work Context come from verified Hub identity. Core's one
UsageVia names the immediate acting server and its immediate Task, if any; opaque Tasks
Core cannot represent reject before reservation. Earlier originating Task constraints
remain in the signed SDK/Hub chain and are not relabeled under the immediate actor.
Full actor history is Hub/SDK provenance, beyond Core's single-via usage record.

## Qualification boundaries

Component tests use explicitly synthetic vectors/media metadata and a simulated
Artifact grant/denial handler. Native SDK Mesh tests prove direct/composed delegation,
permission checks, ordered resolution and no runtime/usage on refusal; they do not
prove actual Artifact SQL ACL or real inference. Artifact's own tests and deployed
real Artifact probes supply that separate evidence. No content is persisted here.

The ignored acceptance runner consumes a new external corpus/capacity manifest with
public inputs, exact Artifact bytes/hash/recipes, optional expected tokens and explicit
ranking expectations. It reserves a new report, validates offline input/baseline before
live entry, reads safe Artifact resources (no signed transfers), makes serial native
embedding calls, validates whole outputs and exact own-usage deltas, and records timings,
request IDs, output identities and compact structured-result byte hashes. Own daily
usage does not establish full request/delegation attribution. Cross-device parity
requires matching processing and source bytes, even if Artifact IDs differ. Real-media
repeated-modality cases, deployed composed calls and quality rankings remain explicit
coordinator scenarios, never inferred from synthetic tests. An external retained Python
wire fixture can exercise the Rust producer/consumer boundary without another model call.


Media coverage deserialization preserves native JSON decimal values through a closed
RawValue payload followed by typed coverage decoding. This avoids serde's internally
tagged buffering interaction with the shared arbitrary_precision feature; neither
alternate wire forms nor handwritten number conversion are accepted. Tests cover raw
JSON and the SDK's structuredContent Value path, including actual retained Python output.
See [acceptance inputs](tests/ACCEPTANCE.md) for the opt-in manifest and temporary native
caller lifecycle. Deployed ACL denial is exercised only with an externally confirmed
existing object outside the caller's actual ACL; otherwise it is explicitly not exercised.
