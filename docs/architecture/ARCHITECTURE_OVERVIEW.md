# ZenFabrique — Architecture Overview

See [ARCHITECTURE_DECISIONS.md](ARCHITECTURE_DECISIONS.md) for the Core vs. Extended stack split and per-component technology choices, and [../planning/STATUS.md](../planning/STATUS.md) for live implementation progress. This doc describes the conceptual shape of the system: the three-plane architecture, separating the "Brain," the "Muscle," and the "Conscience."

## 1. The Control Plane (The Brain)
* **Role:** Defines the truth.
* **Tech:** Apache Jena / Stardog (RDF/OWL).
* **Function:** Stores the business logic as an ontology. Every data event is validated against **SHACL (Shapes Constraint Language)** shapes. This layer is where we perform SPARQL reasoning to understand the semantic relationships between streaming events.

## 2. The Data Plane (The Muscle)
* **Role:** Handles data movement and transformation.
* **Tech:** DuckDB (WASM/Process) & Trino.
* **Function:** Executes zero-copy analytics. When schemas drift, this layer dynamically generates "Shims" (virtualized SQL views) to map new data formats back to the expected ontology without re-writing the physical storage.

## 3. The Policy Plane (The Conscience)
* **Role:** Enforces security and zero-trust.
* **Tech:** Open Policy Agent (OPA).
* **Function:** Decides who—or what—can perform schema evolution or access raw PII. Policies are written in Rego and dynamically evaluated at runtime.
