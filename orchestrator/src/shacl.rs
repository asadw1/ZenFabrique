use anyhow::{Context, Result};
use base64::Engine;

const STAGING_GRAPH: &str = "http://zenfabrique.io/graph/staging";

pub struct ShaclClient {
    base_url: String,
    shapes: String,
    auth_header: String,
}

impl ShaclClient {
    pub fn new(base_url: String, shapes: String, username: &str, password: &str) -> Self {
        let creds = format!("{username}:{password}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
        Self {
            base_url,
            shapes,
            auth_header: format!("Basic {encoded}"),
        }
    }

    // Loads the event's RDF into a scratch graph, then asks Fuseki's SHACL
    // endpoint to validate that graph against the streaming-event-shape
    // contract. Returns whether it conforms.
    pub fn validate(&self, turtle_data: &str) -> Result<bool> {
        let data_url = format!("{}/data?graph={}", self.base_url, STAGING_GRAPH);
        ureq::put(&data_url)
            .set("Content-Type", "text/turtle")
            .set("Authorization", &self.auth_header)
            .send_string(turtle_data)
            .context("failed to load event graph into Fuseki")?;

        let shacl_url = format!("{}/shacl?graph={}", self.base_url, STAGING_GRAPH);
        let response = ureq::post(&shacl_url)
            .set("Content-Type", "text/turtle")
            .set("Accept", "application/n-triples")
            .set("Authorization", &self.auth_header)
            .send_string(&self.shapes)
            .context("failed to call Fuseki SHACL validation endpoint")?
            .into_string()
            .context("failed to read SHACL validation report")?;

        Ok(parse_conforms(&response))
    }
}

fn parse_conforms(ntriples: &str) -> bool {
    ntriples
        .lines()
        .find(|line| line.contains("shacl#conforms"))
        .map(|line| line.contains("\"true\""))
        .unwrap_or(false)
}

// D1/D2 coverage (docs/testing/self-healing-adversarial-matrix.md): the
// matcher is name-only, so these confirm what SHACL itself does and doesn't
// catch about value *content* once a name has matched. Both need a live
// Fuseki with the zenfabrique dataset + shapes loaded — run explicitly with
// `cargo test -- --ignored` after `docker compose up -d jena` and reloading
// the ontology/shapes graphs (see docs/planning/STATUS.md for the curl
// commands).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::{self, Extracted};
    use std::collections::{HashMap, HashSet};

    fn client() -> ShaclClient {
        let shapes_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../control-plane/shacl/streaming-event-shape.ttl");
        let shapes = std::fs::read_to_string(&shapes_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", shapes_path.display()));
        ShaclClient::new(
            "http://localhost:3030/zenfabrique".to_string(),
            shapes,
            "admin",
            "admin",
        )
    }

    fn extracted_with(values: &[(&str, &str)]) -> Extracted {
        let mut values_map = HashMap::new();
        for (k, v) in values {
            values_map.insert(k.to_string(), v.to_string());
        }
        let missing_required = rdf::REQUIRED_FIELDS
            .iter()
            .filter(|f| !values_map.contains_key(**f))
            .map(|f| f.to_string())
            .collect();
        Extracted {
            values: values_map,
            resolved_via: HashMap::new(),
            used_raw_keys: HashSet::new(),
            missing_required,
        }
    }

    // D1 — a coincidental name match heals structurally even with a
    // semantically-garbage value, because the shape only constrains
    // presence/shape, not plausibility (userId has no format constraint
    // beyond non-empty).
    #[test]
    #[ignore = "requires live Fuseki at localhost:3030 with the zenfabrique dataset"]
    fn d1_garbage_value_with_matching_name_still_conforms() {
        let shacl = client();

        let empty = extracted_with(&[
            ("eventId", "evt-d1-empty"),
            ("userId", ""),
            ("trackId", "t1"),
            ("timestamp", "2026-01-01T00:00:00"),
        ]);
        let turtle_empty = rdf::build_turtle("evt-d1-empty", &empty);
        assert!(
            !shacl.validate(&turtle_empty).unwrap(),
            "empty userId should fail sh:minLength 1"
        );

        let garbage = extracted_with(&[
            ("eventId", "evt-d1-garbage"),
            ("userId", "!!!not-an-id###"),
            ("trackId", "t1"),
            ("timestamp", "2026-01-01T00:00:00"),
        ]);
        let turtle_garbage = rdf::build_turtle("evt-d1-garbage", &garbage);
        assert!(
            shacl.validate(&turtle_garbage).unwrap(),
            "non-empty garbage userId has no format constraint, so it conforms — \
             this documents that a successful name match says nothing about \
             value plausibility"
        );
    }

    // D2 — a non-numeric msPlayed value should fail the xsd:nonNegativeInteger
    // datatype constraint's lexical-validity check.
    #[test]
    #[ignore = "requires live Fuseki at localhost:3030 with the zenfabrique dataset"]
    fn d2_non_numeric_ms_played_is_lexically_invalid() {
        let shacl = client();
        let extracted = extracted_with(&[
            ("eventId", "evt-d2"),
            ("userId", "u1"),
            ("trackId", "t1"),
            ("timestamp", "2026-01-01T00:00:00"),
            ("msPlayed", "three minutes"),
        ]);
        let turtle = rdf::build_turtle("evt-d2", &extracted);
        assert!(
            !shacl.validate(&turtle).unwrap(),
            "non-numeric msPlayed should fail the xsd:nonNegativeInteger datatype constraint"
        );
    }
}
