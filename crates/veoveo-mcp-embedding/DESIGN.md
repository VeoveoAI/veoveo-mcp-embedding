# Multimodal embedding generation

This service owns typed MCP discovery, permissions and Core usage. The private runtime
owns bounded media transfer/decoding and the full pinned GPU model. Media arrives as the
caller's signed GET: whoever holds the media, such as Artifact, decided access when it
signed it, so Embedding authorizes no media and calls no other server. No vector
storage, search, pgvector, custom sampler, clipping/chunking/aggregation, model choice or
compatibility route is added. The `client` feature implements the SDK's `Embedder`, so a
knowledge source embeds through this server.

## Public ordered API

`embed` requires `embedding.embeddings.create`; capabilities/docs require
`embedding.models.read`. Ready SDK routes own discovery, authorization and dispatch.
The contract-only feature contains no network, database or model runtime dependencies,
and no Artifact crate. An item's parts are the SDK's knowledge `ContentPart`s, 1 to 16
per item: text, or an image, audio or video carrying the SDK's `SignedMedia` beside its
`type`: the signed `url`, the `headers` its signature binds, `mediaType` and `size` in
bytes. A media part's type is of its kind (an image `image/*`, a video `video/*`, audio
`audio/*` or a video's soundtrack, `video/*`), which the SDK admits on decoding.

```json
{
  "items": [
    {"content": [{"type": "text", "text": "hello again"}]},
    {"content": [
      {"type": "text", "text": "A product demonstration"},
      {"type": "video", "url": "<signed GET URL>", "mediaType": "video/mp4", "size": 7340032}
    ]}
  ],
  "dimensions": 256
}
```

A caller that embeds an Artifact first calls `artifact__create_download` and passes the
URL, its headers, the media type and the size it answers. A signed GET lives five
minutes, so a caller signs right before each call. The URL and headers are the caller's
credential: Embedding passes them only to its private runtime and never echoes them.

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
text/prompt, 64 MiB/source and 128 MiB aggregate transfer, both checked on the caller's
stated `size` before any transfer and again on the actual bytes by the runtime; 16 MP/image or frame and 64 MP
aggregate selected pixels; 300 seconds/source and 600 seconds aggregate decoded audio; video fps ≤ 120
and maxFrames ≤ 256. These are defensive limits, not Google capability or
semantic-quality guarantees. Actual native tokens
must fit 8192/item and 16384/batch; all preflight precedes the first model forward.

## Skills

How an agent uses the server is in its skill `embed-content`, under `skills/`, embedded at
build time and served at `skill://embedding/embed-content/SKILL.md` through the SDK's MCP
Skills support (SEP-2640): items and parts, dimensions, prompt names, preprocessing, the
answer and the limits, naming the tool as callers see it, `embedding__embed`. The
server's `instructions` say what it is and name the skill's address. The skill file is a
resource of the server under its document policy, `embedding.models.read`.

## Signed media and private runtime boundary

Embedding declares no use of another server and holds no media authority. The holder
of the media decided access when it signed the GET; Embedding checks the part's media
type family and the stated size bounds, then hands the signed GET to its runtime. No
bulk bytes pass through MCP, and a request that names media Embedding cannot fetch
fails at the runtime as a closed `source_*` rejection with zero actual usage.

The private server-only DTO carries each media part's `mediaType`, `size` and the
caller's signed GET, `{url, headers}`; empty headers are omitted. There is no
Artifact identity. URLs/headers appear in the public input schema, which must carry
them, and nowhere else: never in output schemas, results, errors, Debug output or
logs. Private serialized payload also must fit 512 KiB, as must the public request.
Runtime independently enforces its configured storage origin, no redirects/proxies,
elapsed deadline, exact raw byte size against the stated size and native decoded
bounds; storage refusing an expired or tampered signature is `source_refused`.
Size-checked temporary files remain owned by its actual worker. Metadata validation
and repeated bounded native decodes are explicit runtime costs.

One 60-second deadline covers the call. Once generation begins, timeout
conservatively reports execution/accounting may be incomplete.
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

Closed image/audio/video coverage identifies each media part by its position, the
item's `index` and its `partIndex`, with the `source` the caller stated, `mediaType`
and `size`; no URL or Artifact identity. Each row must match the exact ordered media
part at that position. Pixel/duration/frame/sample bounds, native option effects,
truncation/retained samples and soundtrack presence are checked. Native repeated frame
indices are valid: order is nondecreasing, not unique. Model prompt timestamps equal
index/sourceFPS; actual codec PTS are separate and can have source timeline offsets.
Validation does not reimplement Google's sampler or independently infer semantics from
pixels. Typed public result validation works on a `RequestShape`, the request without
its signed URLs and headers: each part's text, or its modality, media type and size, and
the request-wide options. Saved receipts keep the shape, never a signed URL.

## Accounting and native composition

Core UsageStore/UsageWrite support synchronous task=None usage. Reserve
min(N × 8192, 16384) as an estimate before runtime dispatch, never as measured tokens.
Validated success supplies actual tokens. A closed correctly status/request-bound
not_started runtime rejection records zero actual. Unknown bodies/codes, disconnect,
timeout or malformed output retain estimate as ambiguous. Accounting failure after
successful inference explicitly says inference completed; never replay it.

Caller tenant and principal come from verified Hub identity. Core's one
UsageVia names the immediate acting server and its immediate Task, if any; opaque Tasks
Core cannot represent reject before reservation. Earlier originating Task constraints
remain in the signed SDK/Hub chain and are not relabeled under the immediate actor.
Full actor history is Hub/SDK provenance, beyond Core's single-via usage record.

## Embedder

The `client` feature, the contract plus the SDK's `client`, holds `EmbeddingClient`,
which implements the SDK's knowledge `Embedder`: `embed(client, items, purpose)` calls
the typed `embed` tool through the caller-bound `HubClient` the knowledge source
already holds, so the caller embeds and the usage is theirs. `EmbeddingPurpose::Document`
uses promptName `Document`, `Query` uses `SearchQuery`, both at 768 dimensions with
default preprocessing. The items must fit one request's bounds; the client refuses
items that do not before calling. It returns `Embeddings` whose model is the checkpoint
the result reports, `{id}@{revision}`, and its dimensions, one vector per item in order,
and refuses a result whose cardinality or order differs. The `server` feature includes
`client`, so the server's tests exercise it.

## Verification

Component tests use explicitly synthetic vectors, media metadata and signed URLs.
Native SDK Mesh tests prove direct and composed calls, permission checks, bounds on the
stated media size and media-type family before any runtime or usage, that the runtime
DTO carries the signed GET without an Artifact identity, that coverage maps by position,
and that a signed URL never reaches results, errors, Debug output or logs. An
`Embedder` test calls through a probe server's caller-bound client to a synthetic
runtime. None of them proves real inference or media validity. No content is persisted
here.

The ignored acceptance runner consumes a new external corpus/capacity manifest whose
media parts name fixture Artifacts, with independently checked fixture bytes/hash/recipes,
optional expected tokens and explicit ranking expectations. It reserves a new report,
validates offline input/baseline before live entry, then, as the caller, signs each
fixture's download through `artifact__create_download`, independently fingerprints the
bounded bytes, and passes the signed media to serial native embedding calls. It validates
whole outputs and exact own-usage deltas, and records timings, request IDs, request
shapes, output identities and compact structured-result byte hashes; signed URLs never
enter receipts. Own daily usage does not establish full request/delegation attribution.
Cross-device parity requires matching processing and source bytes, even if Artifact IDs
differ. An external retained Python wire fixture can exercise the Rust
producer/consumer boundary without another model call.


Media coverage deserialization preserves native JSON decimal values through a closed
RawValue payload followed by typed coverage decoding. This avoids serde's internally
tagged buffering interaction with the shared arbitrary_precision feature; neither
alternate wire forms nor handwritten number conversion are accepted. Tests cover raw
JSON and the SDK's structuredContent Value path, including actual retained Python output.
See [acceptance inputs](tests/ACCEPTANCE.md) for the opt-in manifest and temporary native
caller lifecycle. Artifact ACL denial is Artifact's, at signing, and is qualified there.
