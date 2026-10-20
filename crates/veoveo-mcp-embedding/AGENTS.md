# Embedding server contract

Read DESIGN.md. Slug/schema/scheme `embedding`, MCP `/embedding/mcp`, private port 8803.

- `embed` is a bounded synchronous typed tool: ordered items of ordered text/image/audio/video parts; one joint vector per item. Fixed full EmbeddingGemma 2 revision, optional request-wide prompt and 128/256/512/768 dimensions. Normalized FP32 MPS/CUDA inference, native batch_size=1. No Tasks or persisted products.
- Media parts use native ArtifactId only. Declare Artifact read use and call typed CreateDownload through ctx.client()/native ServerCalls. Artifact owns ACL/availability; MCP never fetches media bytes. Runtime alone consumes private grants and verifies ephemeral bytes. Never expose or Debug/log signed transfers.
- Closed native preprocessing controls mirror the pinned runtime; audio defaults truncation false. Custom clips/chunking/aggregation are excluded. Limits are Veoveo admission bounds, not Google capabilities/quality claims.
- Validate public bounds, all authorized Artifact metadata, aggregate transfer bytes and actual private JSON 512 KiB before usage reservation/dispatch. One 60-second deadline covers resolution and generation. Text-only requests call no Artifact tool.
- Independently validate complete runtime/package/device identity, RFC8785/JCS effective processing hash, ordered cardinality/dimensions/norm/tokens and typed media coverage/source identities. Repeated native video indices are allowed; actualPTS and model index/FPS timestamps differ. Reject partial output.
- Content is transient. Only native Core process/usage metadata persists. Domain schema is empty: no SQL or content tables. Reserve a conservative estimate before inference; confirmed rejection records zero actual; ambiguous execution retains estimate. Accounting failure after inference explicitly says inference completed, with no replay.
- Native UsageVia records the immediate actor/task only. Prior originating Task constraints remain in signed Hub delegation context; do not relabel them under the immediate actor or change Core schema.
- Preserve fail-closed permissions, private provider custody, and contract-only dependency isolation. No priced provider charge means no billed risk or invented price.
- Synthetic vectors/metadata/Artifact denial fixtures prove component boundaries, never inference, encoded media validity or real Artifact ACL. Real MPS/CUDA, native Artifact ACL and deployed acceptance belong to separate receipts. Builder owns full checks/deployment; no provider calls during iteration.
