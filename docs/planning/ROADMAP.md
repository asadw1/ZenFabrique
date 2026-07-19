# ZenFabrique Project Roadmap

This roadmap outlines the development lifecycle for ZenFabrique, focusing on the three-plane architecture (Control, Data, and Policy) to move from conceptual prototype to a functional "nervous system" for data.

## Two-Track Strategy

Phases 1-3 are the **Thin Vertical Slice** (Core MVP): prove the full Observe -> Reason -> Act loop end-to-end using the smallest viable stack (Jena/SHACL, Rust, DuckDB) before adding anything else. Live progress is tracked in [STATUS.md](STATUS.md).

Phases 4-7 integrate the **Extended Architecture** (RabbitMQ, OPA, FHE, Control Room UI, Trino, Dagster). These remain the project's long-term target state — see [ARCHITECTURE_DECISIONS.md](../architecture/ARCHITECTURE_DECISIONS.md) for the Core vs. Extended stack split and the per-component phase assignment. Code in Phases 1-3 was written with decoupled interfaces so these components can be swapped in without rework (e.g., the ingestion listener exposes a single "new event" channel so Phase 4 can point it at RabbitMQ instead of a filesystem watch without touching validation/shim logic).

## Phase Overview

| Phase | Duration | Focus Area | Key Deliverables |
| :--- | :--- | :--- | :--- |
| **1: Foundation** | Weeks 1-2 | Semantics & Ontology | Define `StreamingEvent` ontology (OWL/Turtle); Jena/Fuseki instance; SHACL constraint shapes. |
| **2: Orchestration** | Weeks 3-4 | Rust Core | Build the Rust service; implement event loop and logging; Docker dev environment setup. |
| **3: Self-Healing** | Weeks 5-6 | Logic & Repair | Implement SHACL validation trigger; build "Shim" generator (DuckDB SQL views); automate repair cycle. |
| **4: Transport** | Weeks 7-8 | RabbitMQ | Replace file-watch mock ingestion with a real message broker, behind the existing decoupled ingestion interface. |
| **5: Security** | Weeks 9-10 | OPA & FHE | Integrate OPA for Policy-as-Code; implement FHE/SMPC baseline for PII field encryption. |
| **6: Visibility** | Weeks 11-12 | Control Room UI | Svelte dashboard; integrate Cytoscape.js for topology; connect WebSockets for telemetry. |
| **7: Federation & Hardening** | Weeks 13-14 | Trino, Dagster, Polish | Cross-source query federation; asset-aware orchestration; end-to-end stress testing; latency tuning; documentation. |

---

## Detailed Roadmap

### Phase 1: Foundation (The Brain)
* **Goal:** Establish the source of truth.
* **Tasks:** * Design the core `StreamingEvent` class (User, Track, Timestamp).
    * Configure Apache Jena/Fuseki.
    * Draft initial SHACL shapes to define the "Contract."

### Phase 2: Orchestration (The Nervous System)
* **Goal:** Build the listener.
* **Tasks:** * Create a Rust service that accepts incoming JSON streams.
    * **MVP ingestion:** start with a mock transport — watch a local `events/input/` directory for JSON files — rather than standing up a message broker.
    * Implement a robust logging interface.
    * Define the Docker configuration for local orchestration.
* **Note:** RabbitMQ is the target transport for Phase 4 of the Extended Architecture (see [ARCHITECTURE_DECISIONS.md](../architecture/ARCHITECTURE_DECISIONS.md)) and will replace file-watch ingestion now that the vertical slice is proven. Keep the ingestion interface decoupled from the transport so this swap doesn't require touching downstream logic.

### Phase 3: The Self-Healing Logic (The Actuator)
* **Goal:** Automate schema repair.
* **Tasks:** * Develop the SHACL validator module.
    * Create the "Shim" generator—a logic block that produces SQL views in DuckDB to handle schema discrepancies.
    * Test the automated "break-and-repair" cycle.

### Phase 4: Transport (The Real Nervous System)
* **Goal:** Replace the file-watch mock transport with a real message broker, now that Phases 1-3 have proven the core loop.
* **Tasks:** * Stand up RabbitMQ (Docker).
    * Implement a RabbitMQ consumer behind the existing ingestion channel interface — a swap, not a rewrite, of validation/shim logic downstream.
    * Update `config/fabric.yaml` with broker connection details.
    * Verify the self-healing loop still works end-to-end against the new transport.
    * Decide the fate of file-watch ingestion (retire vs. keep as a dev-mode fallback).

### Phase 5: Policy & Privacy (The Conscience & Shield)
* **Goal:** Integrate Zero-Trust and Privacy-as-Code.
* **Tasks:** * Define basic OPA policies in Rego (e.g., service access control).
    * Implement the FHE library (e.g., OpenFHE) to demonstrate encrypted aggregation on user IDs.

### Phase 6: The Control Room (The Interface)
* **Goal:** Provide diagnostic transparency.
* **Tasks:** * Scaffold the Svelte dashboard.
    * Integrate Cytoscape.js for real-time visualization of the Knowledge Graph and "Shim" nodes.
    * Establish WebSocket connections for real-time fabric telemetry.

### Phase 7: Federation, Orchestration & Hardening
* **Goal:** Scale out the Data Plane and harden the whole fabric for production-shaped load.
* **Tasks:** * Add Trino for cross-source query federation.
    * Add Dagster (OSS core) for asset-aware orchestration and lineage.
    * Migrate storage to Parquet/Delta Lake where applicable.
    * Stress test the Fabric with high-velocity schema mutation scenarios.
    * Optimize DuckDB shim latency and address the single-staging-graph/sequential-processing bottleneck flagged during Phase 3 hardening (see STATUS.md).
    * Finalize documentation and project cleanup.