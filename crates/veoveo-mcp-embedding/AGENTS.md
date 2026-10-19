# Embedding server contract

Read DESIGN.md. Slug/schema/scheme `embedding`, MCP `/embedding/mcp`, private port 8803.

- `embed` is a bounded synchronous typed tool. Fixed EmbeddingGemma 2 revision and processing identity; 768 dimensions, normalized FP32 GPU execution. No Tasks or persisted products.
- Purposes are search_query, code_query and document. Runtime applies exact upstream prompts and validates actual untruncated tokenizer lengths.
- Validate complete runtime identity, bytes, dimensions, order, finite norms and token accounting before exposing output. Reject partial batches.
- Content is transient. Only native Core process/usage metadata persists. The domain schema is empty; no SQL statements or content tables.
- Native forwarded identity supplies usage tenant/principal/context. Record a conservative estimate before dispatch; confirmed rejection records zero actual; ambiguous execution retains estimate. Accounting failure after inference must explicitly say inference completed, without replaying it.
- Preserve fail-closed permissions and private provider custody. No priced-provider charge means no billed-risk or invented price.
- Tests use synthetic vectors to exercise boundaries; they never prove actual model inference. Real MPS/CUDA results belong in separate qualification receipts.
