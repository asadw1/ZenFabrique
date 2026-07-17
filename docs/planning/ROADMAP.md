# ZenFabrique Project Roadmap

This roadmap outlines the development lifecycle for ZenFabrique, focusing on the three-plane architecture (Control, Data, and Policy) to move from conceptual prototype to a functional "nervous system" for data.

## Two-Track Strategy

Phases 1-3 are the **Thin Vertical Slice** (Core MVP): prove the full Observe -> Reason -> Act loop end-to-end using the smallest viable stack (Jena/SHACL, Rust, DuckDB) before adding anything else. Live progress is tracked in [STATUS.md](STATUS.md).

Phases 4-6 integrate the **Extended Architecture** (Trino, Dagster, RabbitMQ, OPA, FHE, Control Room UI). These remain the project's long-term target state — see [ARCHITECTURE_DECISIONS.md](../architecture/ARCHITECTURE_DECISIONS.md) for the Core vs. Extended stack split. Code in Phases 1-3 should be written with decoupled interfaces so these components can be swapped in later without rework (e.g., the ingestion listener should be easy to point at RabbitMQ instead of a filesystem watch).

## Phase Overview

| Phase | Duration | Focus Area | Key Deliverables |
| :--- | :--- | :--- | :--- |
| **1: Foundation** | Weeks 1-2 | Semantics & Ontology | Define `StreamingEvent` ontology (OWL/Turtle); Jena/Fuseki instance; SHACL constraint shapes. |
| **2: Orchestration** | Weeks 3-4 | Rust Core | Build the Rust service; implement event loop and logging; Docker dev environment setup. |
| **3: Self-Healing** | Weeks 5-6 | Logic & Repair | Implement SHACL validation trigger; build "Shim" generator (DuckDB SQL views); automate repair cycle. |
| **4: Security** | Weeks 7-8 | OPA & FHE | Integrate OPA for Policy-as-Code; implement FHE/SMPC baseline for PII field encryption. |
| **5: Visibility** | Weeks 9-10 | Control Room UI | Svelte dashboard; integrate Cytoscape.js for topology; connect WebSockets for telemetry. |
| **6: Integration** | Weeks 11-12 | Polish & Test | End-to-end stress testing (simulating schema drift); latency tuning; documentation. |

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
* **Note:** RabbitMQ is the target transport for the Extended Architecture (see [ARCHITECTURE_DECISIONS.md](../architecture/ARCHITECTURE_DECISIONS.md)) and will replace file-watch ingestion once the vertical slice is proven. Keep the ingestion interface decoupled from the transport so this swap doesn't require touching downstream logic.

### Phase 3: The Self-Healing Logic (The Actuator)
* **Goal:** Automate schema repair.
* **Tasks:** * Develop the SHACL validator module.
    * Create the "Shim" generator—a logic block that produces SQL views in DuckDB to handle schema discrepancies.
    * Test the automated "break-and-repair" cycle.

### Phase 4: Policy & Privacy (The Conscience & Shield)
* **Goal:** Integrate Zero-Trust and Privacy-as-Code.
* **Tasks:** * Define basic OPA policies in Rego (e.g., service access control).
    * Implement the FHE library (e.g., OpenFHE) to demonstrate encrypted aggregation on user IDs.

### Phase 5: The Control Room (The Interface)
* **Goal:** Provide diagnostic transparency.
* **Tasks:** * Scaffold the Svelte dashboard.
    * Integrate Cytoscape.js for real-time visualization of the Knowledge Graph and "Shim" nodes.
    * Establish WebSocket connections for real-time fabric telemetry.

### Phase 6: Hardening
* **Goal:** Performance and stability.
* **Tasks:** * Stress test the Fabric with high-velocity schema mutation scenarios.
    * Optimize DuckDB shim latency.
    * Finalize documentation and project cleanup.