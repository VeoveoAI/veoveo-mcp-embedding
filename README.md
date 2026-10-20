# veoveo-mcp-embedding

Standalone bounded embedding generation through Veoveo Hub and an authenticated private GPU runtime. This Rust service owns native registration, permissions and Core usage; the private runtime owns GPU inference. Read [DESIGN](crates/veoveo-mcp-embedding/DESIGN.md) for the component contract and the [architecture](https://github.com/VeoveoAI/veoveo-docs/blob/main/LORE_OPENSHELL_INTEGRATION.md#embedding-generation) for platform responsibilities.

The multimodal replacement accepts ordered batches of ordered text/image/audio/video parts, with one normalized vector per item. Media references are native Artifact IDs. The MCP authorizes them through typed Artifact calls; the private runtime verifies and decodes the media. Optional dimensions, prompt and closed native preprocessing controls are documented in [DESIGN](crates/veoveo-mcp-embedding/DESIGN.md).

Previous text-only deployment qualification does not qualify this replacement. Component tests, real runtime inference and deployed Hub acceptance are separate gates; the [execution plan](https://github.com/VeoveoAI/veoveo-docs/blob/main/OPENSHELL_HARDCUT_PLAN.md) records their current evidence and limitations. Synthetic test vectors never prove inference or media validity.

## Check

```sh
cargo test --locked --no-default-features --features contract
cargo test --locked --all-features
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
docker build --target check .
```

Tests using the native Core fixture require PostgreSQL 18.6 programs on PATH, as the pinned dev image provides. Contract dependencies exclude tokio, reqwest, rmcp and sqlx. Full checks/builds belong to the builder during coordinated work.

## Configure and run

The private MCP service uses port 8803, slug embedding and `/embedding/mcp`. Normal SDK registration issues its database login; persist only its registration-key directory. Configure these through trusted installation state:

- VEOVEO_HUB_ISSUER, VEOVEO_PUBLIC_BASE_URL, VEOVEO_REGISTRATION_SECRET (native opaque placeholder), VEOVEO_KEY_DIRECTORY, approved host authorities.
- VEOVEO_EMBEDDING_RUNTIME_URL: fixed private origin, initially `http://host.openshell.internal:8804` only after native loopback reachability is qualified.
- VEOVEO_EMBEDDING_RUNTIME_TOKEN: native provider placeholder, never a real token in the author workload.
- VEOVEO_EMBEDDING_DEVICE: explicit mps or cuda; no fallback.

```sh
cargo run --locked --bin veoveo-mcp-embedding
```

The runtime's own repository supplies native macOS launchd and Linux Docker packaging. Installation owns path/binary-bound network policy, provider custody and approved mounts. No public model endpoint or OpenShell patch is part of this component. A host endpoint address is not itself reachability proof.

```sh
docker build --target runtime -t veoveo-mcp-embedding:0.1.0   --build-arg VERSION=0.1.0 --build-arg REVISION=<reviewed-commit> .
```

Mac qualification precedes main merge/push and Linux deployment, as explicitly authorized. Linux acceptance remains separate. No pgvector, vector persistence, Knowledge adaptation or priced provider billing is included.

The ignored native acceptance scenarios and their closed external manifests are documented in [ACCEPTANCE](crates/veoveo-mcp-embedding/tests/ACCEPTANCE.md). They do not run in ordinary checks.
