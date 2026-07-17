# ZenFabrique — Status

Tracks progress on the **Thin Vertical Slice** (Two-Track strategy: prove the core loop end-to-end before integrating the Extended Architecture — see [ARCHITECTURE_DECISIONS.md](../architecture/ARCHITECTURE_DECISIONS.md)).

**Vertical slice pipeline:** `File Ingest -> Jena/SHACL Validation -> Rust Orchestrator -> DuckDB Shim Repair`

Last updated: 2026-07-17

## Phase 1 — Foundation (Ontology)
- [ ] Design core `StreamingEvent` class (User, Track, Timestamp) in OWL/Turtle
- [ ] Stand up Apache Jena/Fuseki instance (local, Docker)
- [ ] Draft initial SHACL shapes defining the event "Contract"

## Phase 2 — Orchestration (Rust Core, mock ingestion)
- [ ] Scaffold Rust service (`orchestrator/`)
- [ ] Implement mock ingestion: watch local `events/input/` directory for JSON files
- [ ] Wire event loop: file detected -> read -> forward for validation
- [ ] Implement structured logging

## Phase 3 — Self-Healing (Shim Repair)
- [ ] Implement SHACL validator call from Rust orchestrator to Fuseki
- [ ] Detect schema breach ("Observe -> Reason" complete)
- [ ] Build Shim generator: emit DuckDB SQL view reconciling drifted event to expected shape
- [ ] Automate break-and-repair cycle end-to-end on a seeded bad event
- [ ] **Vertical slice demo:** drop a malformed JSON event into `events/input/`, observe automatic shim repair, confirm DuckDB view reflects healed data

## Deferred (Extended Architecture — not started, tracked here for visibility only)
- [ ] RabbitMQ transport (replaces file-watch ingestion)
- [ ] OPA/Rego policy plane
- [ ] FHE/SMPC privacy layer
- [ ] Trino federation
- [ ] Dagster orchestration
- [ ] Control Room UI (Svelte + Cytoscape.js + WebSockets)

## How to update this file
Check a box when the task is done and demonstrably working (not just code written — run it). Add a dated one-line note below if a task's scope changes.
