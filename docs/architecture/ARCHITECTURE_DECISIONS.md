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

## Extended Stack (Long-Term Vision — Phases 4-6, deferred)

| Component | Technology | License | FOSS? | Notes |
| :--- | :--- | :--- | :--- | :--- |
| Transport | RabbitMQ | Mozilla Public License 2.0 | Yes | Replaces file-watch ingestion once Core loop is proven. |
| Query federation | Trino | Apache 2.0 | Yes | |
| Orchestration/lineage | Dagster (OSS core) | Apache 2.0 | Yes | Avoid Dagster+ (managed/commercial) — OSS core only. |
| Storage format | Apache Parquet / Delta Lake | Apache 2.0 | Yes | |
| Policy engine | Open Policy Agent (OPA) + Rego | Apache 2.0 | Yes | |
| Privacy (FHE) | OpenFHE | BSD 2-Clause | Yes | Default choice — see decision below. |
| Privacy (FHE), alternative | Concrete (Zama) | BSD-3-Clause-Clear | **Needs verification** | Not an OSI-approved license (excludes patent grant); source-available but ambiguous under a strict FOSS reading. Do not adopt without re-confirming against the FOSS constraint at Phase 4 kickoff. |
| Control Room UI framework | Svelte | MIT | Yes | Locked in. |
| Graph visualization | Cytoscape.js | MIT | Yes | |
| Telemetry transport | WebSockets | Protocol (no license) | N/A | |

## Rejected / Flagged Options

- **Stardog** (listed in the original README as a Jena alternative) is **commercial, closed-source software** — it does not satisfy the FOSS constraint. **Decision: dropped.** Jena/Fuseki is the sole Control Plane store, for both the Core and Extended stacks.
- **Concrete (Zama)** — flagged above, not rejected outright. If FHE work in Phase 4 needs Concrete's specific feature set, re-verify its license against the project's FOSS bar before adopting; otherwise default to OpenFHE.

## Design principle: decoupling for hot-swap

Core Stack components that have a planned Extended Stack replacement or addition (ingestion transport, query engine, orchestration) must sit behind a narrow interface in the Rust orchestrator so the Extended component can be swapped in during its roadmap phase without rewriting the Core loop. Concretely: the ingestion listener should expose a single "new event" callback/channel so swapping the filesystem watcher for a RabbitMQ consumer in Phase 4+ touches only the transport module, not the validation/shim logic.
