# Agent instructions

- This standalone Rust MCP follows the native Veoveo template/SDK/Core contracts. Read the crate's AGENTS.md and DESIGN.md.
- Work only in the assigned worktree; preserve concurrent work. The coordinator owns architecture and rollout, the builder owns full checks/builds/deployments. No subagents or model-provider calls during iteration.
- Pin dependencies to reviewed revisions. Keep the `contract` feature free of runtime/network/database dependencies.
- Generate embeddings only. No Knowledge adaptation, vector storage/search, pgvector, compatibility migrations or extra orchestration.
- Use ready typed SDK routes, normal Hub registration/identity and native Core usage APIs. No handwritten MCP hosting or queries into Core tables.
- Never persist or log text, vectors, provider credentials or arbitrary upstream bodies. No automatic inference replay, redirects or CPU/FP16 fallback.
- Keep test logic in Rust; synthetic provider tests must be labelled synthetic. Run the deterministic Docker check on Mac and Linux as assigned; real GPU qualification is a separate gate.
- The explicitly authorized first rollout qualifies Mac, merges/pushes main, then deploys/qualifies Linux. Do not claim Linux acceptance before its measured gates.
- Commit coherent reviewed changes without tool attribution. No GitHub creation or push before the coordinator's handoff.
