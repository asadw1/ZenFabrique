# ZenFabrique — Architecture Decisions

Records the Core (MVP vertical slice) vs. Extended (long-term vision) stack split, and the FOSS-only tooling constraint. See [ROADMAP.md](../planning/ROADMAP.md) for phasing and [STATUS.md](../planning/STATUS.md) for live progress.

**Constraint:** All technology in this project must be Free and Open Source Software with no licensing cost. Every entry below has been checked against its license; anything not clearly FOSS is flagged rather than assumed.

## Core Stack (Thin Vertical Slice — Phases 1-3)

| Component | Technology | License | FOSS? |
| :--- | :--- | :--- | :--- |
| Ontology store | Apache Jena / Fuseki | Apache 2.0 | Yes |
| Constraint validation | SHACL (via Jena's built-in SHACL engine) | Apache 2.0 | Yes |
| Ontology modeling | Protégé | BSD/MPL (Stanford) | Yes |
| Orchestrator | Rust (std toolchain) | MIT / Apache 2.0 (dual) | Yes |
| Ingestion (MVP) | Local filesystem watch (`events/input/`) | N/A — no dependency | Yes |
| Shim engine | DuckDB | MIT | Yes |

**Decision:** Jena/Fuseki is the locked choice for the Control Plane, not Stardog — see below. All other Core Stack choices in the table above are final for the vertical slice; no further "either/or" ambiguity remains within Phases 1-3.

## Extended Stack (Long-Term Vision — Phases 4-7, deferred)

Per-component phase assignment, see [ROADMAP.md](../planning/ROADMAP.md) for what each phase actually delivers:

| Component | Technology | License | FOSS? | Phase | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Transport | RabbitMQ | Mozilla Public License 2.0 | Yes | 4 | Replaces file-watch ingestion now that the Core loop is proven. |
| Policy engine | Open Policy Agent (OPA) + Rego | Apache 2.0 | Yes | 5 | |
| Privacy (FHE) | OpenFHE, run as a Dockerized service (Python bindings) behind an HTTP API — not linked directly into the Rust orchestrator | BSD 2-Clause | Yes | 5 | **Confirmed at Phase 5 kickoff (2026-07-23).** Direct FFI linkage (the original plan) isn't realistic on this project's Windows dev machine: no `cmake`/`vcpkg`, and OpenFHE's Windows/MSVC support is experimental. Runs as a separate Docker service instead, called over HTTP the same way the orchestrator already calls Fuseki — consistent with how Jena and RabbitMQ are integrated (network services, not linked libraries). |
| Privacy (FHE), alternative | Concrete (Zama) / `tfhe-rs` | BSD-3-Clause-Clear | **Rejected (2026-07-23)** | 5 | Not an OSI-approved license (excludes patent grant); source-available but ambiguous under a strict FOSS reading. `tfhe-rs` looked like an easier Rust-native path than OpenFHE but shares this exact license family — adopting it would re-create the problem this flag exists to catch. OpenFHE is the confirmed choice. |
| Control Room UI framework | Svelte 5 (Vite, plain JS — no SvelteKit) | MIT | Yes | 6 | **Live (2026-07-23).** |
| Telemetry transport | WebSockets (`tokio-tungstenite`, own background-thread runtime) | Protocol (no license) / MIT | N/A | 6 | **Live (2026-07-23).** Implemented as a custom `tracing_subscriber::Layer` rather than a call threaded through every call site — see `orchestrator/src/telemetry.rs`. |
| Graph visualization | Cytoscape.js | MIT | Yes | 6 | Deliberately deferred — Phase 6 was scoped console-first, fully tested end-to-end, before taking this on. |
| Query federation | Trino | Apache 2.0 | Yes | 7 | |
| Orchestration/lineage | Dagster (OSS core) | Apache 2.0 | Yes | 7 | Avoid Dagster+ (managed/commercial) — OSS core only. |
| Storage format | Apache Parquet / Delta Lake | Apache 2.0 | Yes | 7 | |

## Rejected / Flagged Options

- **Stardog** (listed in the original README as a Jena alternative) is **commercial, closed-source software** — it does not satisfy the FOSS constraint. **Decision: dropped.** Jena/Fuseki is the sole Control Plane store, for both the Core and Extended stacks.
- **Concrete (Zama) / `tfhe-rs`** — re-verified and rejected at Phase 5 kickoff (2026-07-23): both share the same non-OSI-approved BSD-3-Clause-Clear license. OpenFHE (BSD-2) is the confirmed FHE choice, run as a Dockerized service rather than linked directly into the orchestrator — see the Extended Stack table above.

## Design principle: decoupling for hot-swap

Core Stack components that have a planned Extended Stack replacement or addition (ingestion transport, query engine, orchestration) must sit behind a narrow interface in the Rust orchestrator so the Extended component can be swapped in during its roadmap phase without rewriting the Core loop. Concretely: the ingestion listener should expose a single "new event" callback/channel so swapping the filesystem watcher for a RabbitMQ consumer in Phase 4+ touches only the transport module, not the validation/shim logic.
