use crate::fhe::FheClient;
use crate::ingest::RawEvent;
use crate::opa::OpaClient;
use crate::rdf;
use crate::shacl::ShaclClient;
use crate::shim::ShimEngine;
use anyhow::Result;
use tracing::{info, warn};

// The Observe -> Reason -> Act loop for a single event:
//   1. extract known fields, build RDF, validate against the SHACL contract
//   2. on breach, try to fuzzy-match missing fields against the event's
//      actual keys; a unique candidate still needs an OPA policy check
//      before the shim's alias registry is actually widened (the "repair")
//   3. re-validate to confirm the repair actually worked
//   4. if a usage value (msPlayed) and its owner (userId) both resolved,
//      encrypt the value via FHE and persist only the ciphertext for that
//      metric — independent of SHACL conformance, since msPlayed is optional
//   5. always persist the raw event so the shim view can (retroactively)
//      reflect it once/if it's healed
pub fn process(event: &RawEvent, shacl: &ShaclClient, opa: &OpaClient, fhe: &FheClient, shim: &mut ShimEngine) -> Result<()> {
    let started = std::time::Instant::now();
    let source = event.source.clone();
    let origin = event.origin.clone();
    let payload_text = event.payload.to_string();
    let fallback_id = event.fallback_id.clone();

    let mut extracted = rdf::extract(&event.payload, shim.aliases());

    if !extracted.missing_required.is_empty() {
        warn!(
            source = %source,
            missing = ?extracted.missing_required,
            "schema breach detected — attempting self-healing repair"
        );

        let payload_obj = event.payload.as_object();
        // Cloned so repairs found earlier in this loop are excluded from
        // candidates for later fields in the *same* pass (fixes F2: the set
        // used to go stale mid-loop, which happened to be harmless only
        // because no two canonical fields normalize to the same string).
        let mut used = extracted.used_raw_keys.clone();

        let mut learned_any = false;
        for field in extracted.missing_required.clone() {
            match rdf::find_alias_candidate(&field, payload_obj, &used) {
                rdf::AliasMatch::Unique(raw_key) => {
                    used.insert(raw_key.clone());
                    if !opa.allow_mutation(&origin, &field, &raw_key) {
                        warn!(
                            canonical = %field,
                            raw_key = %raw_key,
                            origin = %origin,
                            "policy denied schema mutation — not healed"
                        );
                        continue;
                    }
                    info!(
                        canonical = %field,
                        raw_key = %raw_key,
                        source = %source,
                        "self-healing: discovered renamed field, widening shim"
                    );
                    if shim.learn_alias(&field, &raw_key)? {
                        learned_any = true;
                    }
                }
                rdf::AliasMatch::Ambiguous(candidates) => {
                    warn!(
                        canonical = %field,
                        candidates = ?candidates,
                        source = %source,
                        "multiple candidate keys with conflicting values — refusing to guess, not healed"
                    );
                }
                rdf::AliasMatch::None => {}
            }
        }

        if learned_any {
            shim.regenerate_view()?;
            extracted = rdf::extract(&event.payload, shim.aliases());
        }
    }

    // Whenever a field resolved via something other than its bare canonical
    // name, a learned alias did the work — record that it was exercised
    // just now (E1: gives an operator a way to notice an alias that's gone
    // stale, since staleness itself can't be detected automatically).
    for (field, raw_key) in &extracted.resolved_via {
        if raw_key != field {
            shim.touch_alias(field, raw_key)?;
        }
    }

    if let (Some(user_id), Some(ms_played_text)) =
        (extracted.values.get("userId"), extracted.values.get("msPlayed"))
    {
        match ms_played_text.parse::<i64>() {
            Ok(ms_played) if ms_played >= 0 => match fhe.encrypt(ms_played) {
                Ok(Some(ciphertext)) => {
                    let event_id = extracted
                        .values
                        .get("eventId")
                        .cloned()
                        .unwrap_or_else(|| fallback_id.clone());
                    if let Err(e) = shim.store_encrypted_metric(&event_id, user_id, &ciphertext) {
                        warn!(source = %source, error = %e, "failed to persist encrypted metric");
                    }
                }
                Ok(None) => {} // no FHE service configured — nothing to store
                Err(e) => warn!(source = %source, error = %e, "failed to encrypt msPlayed via FHE service"),
            },
            _ => {}
        }
    }

    let turtle = rdf::build_turtle(&fallback_id, &extracted);

    let conforms = if extracted.missing_required.is_empty() {
        match shacl.validate(&turtle) {
            Ok(conforms) => conforms,
            Err(e) => {
                warn!(source = %source, error = %e, "SHACL validation call failed");
                false
            }
        }
    } else {
        false
    };

    // Logged on every event rather than sampled — cheap at this volume, and
    // gives an actual number to point at instead of "no measurement yet"
    // for the file-drop-to-decision latency question.
    let duration_ms = started.elapsed().as_millis();

    if conforms {
        info!(source = %source, duration_ms, "event conforms to StreamingEvent contract");
    } else {
        warn!(
            source = %source,
            missing = ?extracted.missing_required,
            duration_ms,
            "event does not conform — stored for audit, not healed"
        );
    }

    shim.insert_raw(&source, &payload_text)?;

    Ok(())
}

// Resilience, audit-completeness, and cross-event-leakage coverage (the
// three remaining "how do we know the vertical slice is proven" gaps that
// the 16-scenario adversarial matrix didn't touch, since that pass was
// about the matcher's correctness, not the pipeline's operational behavior).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::opa::OpaClient;
    use crate::shim::ShimEngine;
    use serde_json::json;
    use std::path::PathBuf;

    fn temp_db_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("zenfabrique_test_validate_{name}_{nanos}.duckdb"))
    }

    // Port 1 has nothing listening, so connections fail fast (refused)
    // rather than hanging on a timeout — this exercises the real
    // "Fuseki unreachable" error path without needing to actually stop the
    // docker container from a test.
    fn dead_shacl_client() -> ShaclClient {
        ShaclClient::new(
            "http://127.0.0.1:1/zenfabrique".to_string(),
            String::new(),
            "admin",
            "admin",
        )
    }

    fn raw_event(source: &str, payload: serde_json::Value) -> RawEvent {
        let fallback_id = PathBuf::from(source)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        RawEvent {
            source: source.to_string(),
            origin: fallback_id.clone(),
            fallback_id,
            payload,
        }
    }

    #[test]
    fn batch_survives_fuseki_outage_with_full_audit_and_no_leakage() {
        let path = temp_db_path("batch");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
        let shacl = dead_shacl_client();
        // Bypasses policy — this test is about surviving a Fuseki outage
        // with full audit/no-leakage, not about policy gating (see the
        // dedicated policy tests below).
        let opa = OpaClient::disabled();
        let fhe = FheClient::disabled();

        let events = vec![
            raw_event(
                "e1.json",
                json!({"eventId": "e1", "userId": "u1", "trackId": "t1", "timestamp": "2026-01-01T00:00:00"}),
            ),
            // repaired via a different alias per field, to catch any
            // accidental sharing of state between events
            raw_event(
                "e2.json",
                json!({"eventId": "e2", "user_id": "u2", "trackId": "t2", "timestamp": "2026-01-01T00:01:00"}),
            ),
            raw_event(
                "e3.json",
                json!({"eventId": "e3", "userId": "u3", "track_id": "t3", "timestamp": "2026-01-01T00:02:00"}),
            ),
            // extra/unknown fields alongside otherwise-complete data
            raw_event(
                "e4.json",
                json!({
                    "eventId": "e4", "userId": "u4", "trackId": "t4", "timestamp": "2026-01-01T00:03:00",
                    "debugFlag": true, "requestId": "unrelated-value"
                }),
            ),
            // genuinely unrepairable — no trackId, nothing plausibly renamed
            raw_event(
                "e5.json",
                json!({"eventId": "e5", "userId": "u5", "timestamp": "2026-01-01T00:04:00"}),
            ),
        ];

        for event in &events {
            process(event, &shacl, &opa, &fhe, &mut shim).expect("process() must not error even with Fuseki unreachable");
        }

        // Audit completeness: every event landed exactly once, healed or not.
        assert_eq!(shim.raw_event_count().unwrap(), events.len() as i64);

        // No cross-event leakage: each row must resolve to its OWN values.
        let e1 = shim.query_streaming_event("e1").unwrap().unwrap();
        assert_eq!(e1.user_id, Some("u1".to_string()));
        assert_eq!(e1.track_id, Some("t1".to_string()));

        let e2 = shim.query_streaming_event("e2").unwrap().unwrap();
        assert_eq!(e2.user_id, Some("u2".to_string()));
        assert_eq!(e2.track_id, Some("t2".to_string()));

        let e3 = shim.query_streaming_event("e3").unwrap().unwrap();
        assert_eq!(e3.user_id, Some("u3".to_string()));
        assert_eq!(e3.track_id, Some("t3".to_string()));

        let e4 = shim.query_streaming_event("e4").unwrap().unwrap();
        assert_eq!(e4.user_id, Some("u4".to_string()));
        assert_eq!(e4.track_id, Some("t4".to_string()));

        let e5 = shim.query_streaming_event("e5").unwrap().unwrap();
        assert_eq!(e5.user_id, Some("u5".to_string()));
        assert_eq!(e5.track_id, None, "e5 must not have picked up a neighbor's trackId");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    // A policy engine that's down must not be treated as "policy has no
    // opinion" — fail-closed means the mutation stays unhealed, same as if
    // it had been explicitly denied.
    #[test]
    fn unreachable_opa_leaves_field_unhealed() {
        let path = temp_db_path("policy_unreachable");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
        let shacl = dead_shacl_client();
        let opa = OpaClient::remote("http://127.0.0.1:1");
        let fhe = FheClient::disabled();

        let event = raw_event(
            "e1.json",
            json!({"eventId": "e1", "user_id": "u1", "trackId": "t1", "timestamp": "2026-01-01T00:00:00"}),
        );
        process(&event, &shacl, &opa, &fhe, &mut shim).unwrap();

        let e1 = shim.query_streaming_event("e1").unwrap().unwrap();
        assert_eq!(e1.user_id, None, "fail-closed: an unreachable policy engine must deny, not skip, the check");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    // Requires `docker compose up -d opa` with policy-plane/rego loaded
    // (mounted directly by the compose service, no manual load step).
    #[test]
    #[ignore = "requires live OPA at localhost:8181 with policy-plane/rego loaded"]
    fn policy_gates_protected_field_mutation_by_source_trust() {
        let path = temp_db_path("policy_live");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
        let shacl = dead_shacl_client();
        let opa = OpaClient::remote("http://localhost:8181");
        let fhe = FheClient::disabled();

        // untrusted-source: userId is protected, source isn't trusted -> denied
        let denied = raw_event(
            "untrusted-source",
            json!({"eventId": "e-denied", "user_id": "u1", "trackId": "t1", "timestamp": "2026-01-01T00:00:00"}),
        );
        process(&denied, &shacl, &opa, &fhe, &mut shim).unwrap();
        let e_denied = shim.query_streaming_event("e-denied").unwrap().unwrap();
        assert_eq!(e_denied.user_id, None, "protected field mutation from an untrusted source must be denied");

        // partner-feed: userId is protected, but this source is trusted -> allowed
        let allowed = raw_event(
            "partner-feed",
            json!({"eventId": "e-allowed", "user_id": "u2", "trackId": "t2", "timestamp": "2026-01-01T00:01:00"}),
        );
        process(&allowed, &shacl, &opa, &fhe, &mut shim).unwrap();
        let e_allowed = shim.query_streaming_event("e-allowed").unwrap().unwrap();
        assert_eq!(e_allowed.user_id, Some("u2".to_string()), "protected field mutation from a trusted source must be allowed");

        // untrusted-source: trackId isn't protected -> allowed regardless of source trust
        let unprotected = raw_event(
            "untrusted-source",
            json!({"eventId": "e-unprotected", "userId": "u3", "track_id": "t3", "timestamp": "2026-01-01T00:02:00"}),
        );
        process(&unprotected, &shacl, &opa, &fhe, &mut shim).unwrap();
        let e_unprotected = shim.query_streaming_event("e-unprotected").unwrap().unwrap();
        assert_eq!(e_unprotected.track_id, Some("t3".to_string()), "unprotected field mutation is allowed for any source");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_fhe_stores_no_encrypted_metrics() {
        let path = temp_db_path("fhe_disabled");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
        let shacl = dead_shacl_client();
        let opa = OpaClient::disabled();
        let fhe = FheClient::disabled();

        let event = raw_event(
            "e1.json",
            json!({"eventId": "e1", "userId": "u1", "trackId": "t1", "timestamp": "2026-01-01T00:00:00", "msPlayed": 180000}),
        );
        process(&event, &shacl, &opa, &fhe, &mut shim).unwrap();

        assert!(shim.ciphertexts_for_user("u1").unwrap().is_empty(), "no FHE service configured — nothing should be encrypted");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    // Requires `docker compose up -d fhe`.
    #[test]
    #[ignore = "requires the live FHE service at localhost:8090"]
    fn live_fhe_encrypts_ms_played_at_ingest_and_aggregates_correctly() {
        let path = temp_db_path("fhe_live");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
        let shacl = dead_shacl_client();
        let opa = OpaClient::disabled();
        let fhe = FheClient::remote("http://localhost:8090");

        let events = [
            ("e1", 180_000i64),
            ("e2", 210_000i64),
        ];
        for (event_id, ms_played) in &events {
            let event = raw_event(
                &format!("{event_id}.json"),
                json!({"eventId": event_id, "userId": "u-live", "trackId": "t1", "timestamp": "2026-01-01T00:00:00", "msPlayed": ms_played}),
            );
            process(&event, &shacl, &opa, &fhe, &mut shim).unwrap();
        }

        let ciphertexts = shim.ciphertexts_for_user("u-live").unwrap();
        assert_eq!(ciphertexts.len(), 2, "both events' msPlayed should have been encrypted and stored");

        let sum = fhe.aggregate(&ciphertexts).unwrap();
        let expected: i64 = events.iter().map(|(_, ms)| ms).sum();
        assert_eq!(sum, expected, "the FHE service's aggregate must match the plaintext sum, without this test ever sending it a raw sum to check against");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }
}
