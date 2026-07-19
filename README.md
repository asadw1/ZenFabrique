# ZenFabrique

### Autonomous Semantic Governance for Real-Time Streaming Data

**ZenFabrique** is a prototype nervous system for data. It moves away from static, manual ETL pipelines and towards an autonomous, self-healing architecture where metadata is active, governance is computational, and data sharing is zero-copy.

Built specifically for the music streaming domain, this project explores how a distributed system can detect, reason about, and automatically repair schema breaches without manual intervention.

---

## 📋 Requirements

ZenFabrique is being built as a Core MVP vertical slice first, with the Extended Architecture layered in later (see [docs/architecture/ARCHITECTURE_DECISIONS.md](docs/architecture/ARCHITECTURE_DECISIONS.md)). Install only what the track you're working on needs.

### Required to run the Core MVP (available today)

| Software | Version | Why it's needed |
| :--- | :--- | :--- |
| [Rust](https://www.rust-lang.org/tools/install) (rustup) | Stable, 1.75+ | Builds and runs the orchestrator (`cargo run`). |
| [Docker](https://docs.docker.com/get-docker/) + Docker Compose | 24+ | Runs Apache Jena/Fuseki locally (`docker-compose up -d jena`). |
| [DuckDB](https://duckdb.org/docs/installation/) | 0.10+ | Executes the generated shim SQL views. CLI is enough for manual inspection; the orchestrator links it via the `duckdb` Rust crate. |
| [Git](https://git-scm.com/downloads) | 2.x | Version control (this repo). |

### Additional requirements for the Extended Architecture (not needed yet)

Only install these once you're actually working on their corresponding roadmap phase — see [docs/planning/ROADMAP.md](docs/planning/ROADMAP.md).

| Software | Version | Why it's needed |
| :--- | :--- | :--- |
| [Node.js](https://nodejs.org/) + npm | 20 LTS+ | Builds and runs the Svelte "Control Room" dashboard. |
| [Open Policy Agent (OPA)](https://www.openpolicyagent.org/docs/latest/#running-opa) | 0.60+ | Evaluates Rego policies for the Policy Plane. |
| [RabbitMQ](https://www.rabbitmq.com/docs/download) | 3.13+ (via Docker) | Replaces file-watch ingestion as the real event transport. |
| [Trino](https://trino.io/download.html) | 440+ | Cross-source query federation. |
| Python 3.11+ | — | Required by [Dagster](https://docs.dagster.io/getting-started/install) for asset orchestration. |
| [OpenFHE](https://github.com/openfheorg/openfhe-development) (or [Concrete](https://github.com/zama-ai/concrete) — see license note in [ARCHITECTURE_DECISIONS.md](docs/architecture/ARCHITECTURE_DECISIONS.md)) | Latest | FHE library for privacy-preserving PII aggregation. |

---

## 🛠️ Technology Stack

ZenFabrique is built using a highly decoupled, state-of-the-art open-source stack to demonstrate advanced active metadata and zero-copy paradigms.

### Core Architectural Layers

| Layer | Technology | Primary Role in ZenFabrique |
| :--- | :--- | :--- |
| **Control Plane** | **Apache Jena (Fuseki) / Stardog** | RDF/OWL triple store and graph database hosting the semantic ontology model and executing SPARQL queries. |
| | **SHACL (Shapes Constraint)** | Explicit contract enforcement and constraint validation on streaming payloads to detect schema drift. |
| | **Protégé** | Direct visual modeling of the core music streaming domain entities and relationships. |
| **Data Plane** | **DuckDB (WASM / Native)** | Embedded, high-performance analytical engine for executing zero-copy query translations and dynamic runtime shims. |
| | **Trino** | Distributed SQL query federation engine enabling cross-source analysis without physical data egress. |
| | **Apache Parquet / Delta Lake** | Columnar, version-controlled storage format serving as the underlying data layer. |
| **Policy Plane** | **Open Policy Agent (OPA)** | Policy-as-Code engine using **Rego** to evaluate zero-trust data access, governance, and schema mutation privileges. |
| **Orchestration** | **Rust** | System-level, low-latency agent managing the core event-loop, intercepting schema failures, and spawning self-healing tasks. |
| | **Dagster** | Asset-first orchestrator managing state-aware data workflows, lineage tracking, and job execution. |

### Diagnostic "Control Room" Frontend

* **Svelte:** Light-weight, high-reactivity framework for the UI state.
* **Cytoscape.js:** Graph theory and visualization library used to map the real-time topology of the active ontology and visible shim adaptors.
* **WebSockets:** Bidirectional communication channel pulling active telemetry and validation logs directly from the Rust orchestrator.

### Advanced Cryptography

* **FHE / SMPC Libraries:** (e.g., *OpenFHE* or *Concrete*) Used to implement privacy-preserving math over encrypted user streaming metrics, preventing raw PII leakage to downstream analytics tools.

---

## 🏛️ Architecture Overview

The system is designed as a three-plane architecture, separating the "Brain," the "Muscle," and the "Conscience."

### 1. The Control Plane (The Brain)
* **Role:** Defines the truth.
* **Tech:** Apache Jena / Stardog (RDF/OWL).
* **Function:** Stores the business logic as an ontology. Every data event is validated against **SHACL (Shapes Constraint Language)** shapes. This layer is where we perform SPARQL reasoning to understand the semantic relationships between streaming events.

### 2. The Data Plane (The Muscle)
* **Role:** Handles data movement and transformation.
* **Tech:** DuckDB (WASM/Process) & Trino.
* **Function:** Executes zero-copy analytics. When schemas drift, this layer dynamically generates "Shims" (virtualized SQL views) to map new data formats back to the expected ontology without re-writing the physical storage.

### 3. The Policy Plane (The Conscience)
* **Role:** Enforces security and zero-trust.
* **Tech:** Open Policy Agent (OPA).
* **Function:** Decides who—or what—can perform schema evolution or access raw PII. Policies are written in Rego and dynamically evaluated at runtime.

---

## 🧠 The "Self-Healing" Workflow

The fabric operates on an **Observe -> Reason -> Act** loop:

1.  **Observe:** Incoming streaming events (e.g., "Song Played") are captured.
2.  **Reason:** The system validates the event against the SHACL shape defined in the Knowledge Graph. If the structure is unrecognized (e.g., an unannounced schema change by the Producer service), the system triggers a "breakage" event.
3.  **Act:**
    * The **Rust-based Orchestrator** detects the breach.
    * It queries the Ontology to find the current valid shape.
    * It triggers an autonomous transformation that generates a temporary DuckDB SQL mapping (the "Shim") to normalize the event.
    * The Fabric updates the **GraphQL Federation** schema to allow downstream consumers (e.g., Royalties Engine) to continue receiving consistent data.

---

## 🛡️ Privacy-Preserving Analytics (FHE)
The fabric integrates Privacy-as-Code. User listening history is sensitive PII. Using Homomorphic Encryption (FHE) techniques, the system allows the Analytics Engine to calculate aggregate metrics (e.g., "Total Plays per Artist") across encrypted data. The data is only decrypted at the final output gate, ensuring full compliance without sacrificing analytical utility.

---

## 🖥️ The "Control Room" Dashboard
Unlike standard administrative panels, the **ZenFabrique UI** serves as a diagnostic dashboard for the system's "nervous system."

* **Topology Graph:** Visualizes the Knowledge Graph (using Cytoscape.js) to show current connections and "shim" nodes.
* **Observability Stream:** Real-time logging of schema evolution and autonomous repairs.
* **Policy Gate:** A toggle interface for OPA policies, allowing you to simulate "Zero-Trust" enforcement and observe the system rejecting unauthorized data changes in real-time.

---

## 🚀 Getting Started

> **Status:** ZenFabrique is being built as a "Thin Vertical Slice" first (see [docs/planning/STATUS.md](docs/planning/STATUS.md) for live progress and [docs/architecture/ARCHITECTURE_DECISIONS.md](docs/architecture/ARCHITECTURE_DECISIONS.md) for the Core vs. Extended stack split). The instructions below are split accordingly — the Core MVP steps are what's actually being built right now; the Extended Architecture steps describe the target end-state and are not yet implemented.

### Core MVP Build (current focus)
Proves the Observe -> Reason -> Act loop end-to-end with the smallest viable stack. See [Requirements](#-requirements) above for what to install first.

1.  **Initialize Knowledge Graph:** `docker-compose up -d jena`. The custom Fuseki assembler config (`config/fuseki/zenfabrique.ttl`) creates the `zenfabrique` dataset — with SPARQL query/update, Graph Store, and SHACL validation endpoints — automatically on first boot. No manual data loading is required for the loop below to work; the orchestrator ships its own copy of the SHACL shapes and posts them fresh on every validation call. (Optional: load `control-plane/ontology/streaming-event.ttl` into Fuseki's Graph Store endpoint if you want to browse the ontology via SPARQL — see `docs/planning/STATUS.md` for the exact `curl` commands.)
2.  **Seed mock events:** drop JSON event files into `events/input/` — this stands in for a real message broker until Phase 4.
3.  **Spin up the Nervous System:** run the Rust orchestrator from the repo root:
    ```bash
    cargo run --manifest-path orchestrator/Cargo.toml --release -- --config ./config/fabric.yaml
    ```
4.  **Observe repair:** malformed events dropped in `events/input/` should trigger an automatic DuckDB shim; check the orchestrator logs (each event now logs a `duration_ms`) and query the resulting `streaming_events` view in `data-plane/zenfabrique.duckdb` with the `duckdb` CLI.

### Extended Architecture (target state, not yet implemented)
Everything below is planned but deferred until the vertical slice above is proven — see [docs/planning/ROADMAP.md](docs/planning/ROADMAP.md) Phases 4-6.

* **Transport:** Replace file-watch ingestion with RabbitMQ.
* **Policy Plane:** Deploy OPA with Rego policy bundles for zero-trust schema mutation and access control.
* **Privacy:** Integrate FHE/SMPC for encrypted PII aggregation.
* **Federation:** Add Trino for cross-source query federation; Dagster for asset-aware orchestration.
* **Control Room UI:**
    ```bash
    cd ui && npm install && npm run dev
    ```

---

## 📜 Future Layering
This architecture is modular by design. Future roadmap includes:
* **Federated GraphQL:** To allow unified querying across external data sources.
* **Advanced Logic Reasoning:** Implementing fully automated ontology expansion based on emerging data patterns.
* **Hardened Privacy:** Expanding FHE support to include cross-service SMPC (Secure Multi-Party Computation).

---

*This project is a personal exploration into high-scale, autonomous data systems.*