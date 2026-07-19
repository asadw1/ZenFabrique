use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub const REQUIRED_FIELDS: [&str; 4] = ["eventId", "userId", "trackId", "timestamp"];
pub const OPTIONAL_FIELDS: [&str; 1] = ["msPlayed"];

pub type AliasMap = HashMap<String, Vec<String>>;

pub fn default_aliases() -> AliasMap {
    let mut map = AliasMap::new();
    for field in REQUIRED_FIELDS.iter().chain(OPTIONAL_FIELDS.iter()) {
        map.insert(field.to_string(), vec![field.to_string()]);
    }
    map
}

pub struct Extracted {
    pub values: HashMap<String, String>,
    // Which raw key actually satisfied each found field. Equal to the
    // canonical field name itself when resolved directly; anything else
    // means a learned alias was exercised (used to drive the E1 audit trail
    // — see ShimEngine::touch_alias).
    pub resolved_via: HashMap<String, String>,
    pub used_raw_keys: HashSet<String>,
    pub missing_required: Vec<String>,
}

// Looks up each canonical field via its known aliases (falling back to the
// canonical name itself). Fields that can't be found are recorded in
// `missing_required` rather than failing outright, so a schema-drifted event
// still produces a (partial) RDF graph for SHACL to flag.
pub fn extract(payload: &Value, aliases: &AliasMap) -> Extracted {
    let obj = payload.as_object();
    let mut values = HashMap::new();
    let mut resolved_via = HashMap::new();
    let mut used_raw_keys = HashSet::new();
    let mut missing_required = Vec::new();

    for field in REQUIRED_FIELDS.iter().chain(OPTIONAL_FIELDS.iter()) {
        let candidates = aliases
            .get(*field)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let found = obj.and_then(|obj| {
            candidates.iter().find_map(|key| {
                obj.get(key)
                    .and_then(scalar_to_text)
                    .map(|text| (key.clone(), text))
            })
        });

        match found {
            Some((key, text)) => {
                used_raw_keys.insert(key.clone());
                resolved_via.insert(field.to_string(), key);
                values.insert(field.to_string(), text);
            }
            None if REQUIRED_FIELDS.contains(field) => {
                missing_required.push(field.to_string());
            }
            None => {}
        }
    }

    Extracted {
        values,
        resolved_via,
        used_raw_keys,
        missing_required,
    }
}

fn scalar_to_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

// Renders whatever fields were found as Turtle. Missing required fields are
// simply omitted rather than substituted with a placeholder, so the SHACL
// shape's minCount constraints catch them naturally.
pub fn build_turtle(fallback_event_id: &str, extracted: &Extracted) -> String {
    let event_id = extracted
        .values
        .get("eventId")
        .cloned()
        .unwrap_or_else(|| fallback_event_id.to_string());
    let event_iri_id = sanitize_id(&event_id);

    let mut ttl = String::new();
    ttl.push_str("@prefix zf: <http://zenfabrique.io/ontology#> .\n");
    ttl.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n");

    ttl.push_str(&format!(
        "<http://zenfabrique.io/event/{event_iri_id}> a zf:StreamingEvent"
    ));
    if let Some(v) = extracted.values.get("eventId") {
        ttl.push_str(&format!(" ;\n  zf:eventId \"{}\"", escape_str(v)));
    }
    if let Some(v) = extracted.values.get("timestamp") {
        ttl.push_str(&format!(
            " ;\n  zf:eventTimestamp \"{}\"^^xsd:dateTime",
            escape_str(v)
        ));
    }
    if let Some(v) = extracted.values.get("msPlayed") {
        ttl.push_str(&format!(
            " ;\n  zf:msPlayed \"{}\"^^xsd:nonNegativeInteger",
            escape_str(v)
        ));
    }
    if let Some(v) = extracted.values.get("userId") {
        ttl.push_str(&format!(
            " ;\n  zf:performedBy <http://zenfabrique.io/user/{}>",
            sanitize_id(v)
        ));
    }
    if let Some(v) = extracted.values.get("trackId") {
        ttl.push_str(&format!(
            " ;\n  zf:involvesTrack <http://zenfabrique.io/track/{}>",
            sanitize_id(v)
        ));
    }
    ttl.push_str(" .\n\n");

    if let Some(v) = extracted.values.get("userId") {
        ttl.push_str(&format!(
            "<http://zenfabrique.io/user/{}> a zf:User ; zf:userId \"{}\" .\n",
            sanitize_id(v),
            escape_str(v)
        ));
    }
    if let Some(v) = extracted.values.get("trackId") {
        ttl.push_str(&format!(
            "<http://zenfabrique.io/track/{}> a zf:Track ; zf:trackId \"{}\" .\n",
            sanitize_id(v),
            escape_str(v)
        ));
    }

    ttl
}

fn sanitize_id(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[derive(Debug, Clone, PartialEq)]
pub enum AliasMatch {
    None,
    // Exactly one candidate key, or several that all agree on the same
    // value (safe to heal either way — see adversarial scenario B1).
    Unique(String),
    // Multiple candidate keys with *conflicting* values — silently picking
    // one would write data that might be wrong with no error signal (B2).
    // The caller should refuse to auto-heal and surface this for review.
    Ambiguous(Vec<String>),
}

// Fuzzy-matches a missing canonical field against the event's actual raw
// keys (ignoring keys already claimed by another field) — this is the
// self-healing "reasoning" step: a renamed-but-recognizable field (e.g.
// `user_id` for `userId`) gets discovered without a hardcoded synonym list.
pub fn find_alias_candidate(canonical: &str, obj: Option<&Map<String, Value>>, used: &HashSet<String>) -> AliasMatch {
    let Some(obj) = obj else {
        return AliasMatch::None;
    };

    let target = normalize(canonical);
    let matches: Vec<&String> = obj
        .keys()
        .filter(|k| !used.contains(*k))
        .filter(|k| normalize(k) == target)
        .collect();

    match matches.len() {
        0 => AliasMatch::None,
        1 => AliasMatch::Unique(matches[0].clone()),
        _ => {
            let values: Vec<Option<String>> = matches
                .iter()
                .map(|k| obj.get(*k).and_then(scalar_to_text))
                .collect();
            let first = values[0].clone();
            if values.iter().all(|v| *v == first) {
                AliasMatch::Unique(matches[0].clone())
            } else {
                AliasMatch::Ambiguous(matches.into_iter().cloned().collect())
            }
        }
    }
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

// Adversarial matrix coverage (docs/testing/self-healing-adversarial-matrix.md):
// A1-A4 naming variants, B1-B3 same-field ambiguity, C1-C3 multi-field
// drift, F1 duplicate-key parsing, F2 the used-keys threading fix.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: &Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    // A1 — snake_case rename, the baseline already verified live in Phase 3.
    #[test]
    fn a1_snake_case_rename_discovered() {
        let payload = json!({"user_id": "u1"});
        let o = obj(&payload);
        match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
            AliasMatch::Unique(k) => assert_eq!(k, "user_id"),
            other => panic!("expected Unique(user_id), got {other:?}"),
        }
    }

    // A2 — case variants all normalize identically.
    #[test]
    fn a2_case_variants_all_match() {
        for key in ["USERID", "UserID", "userID"] {
            let payload = json!({ key: "u1" });
            let o = obj(&payload);
            match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
                AliasMatch::Unique(k) => assert_eq!(k, key),
                other => panic!("case variant {key} did not match: {other:?}"),
            }
        }
    }

    // A3 — separator variants (-, ., _) are all stripped before comparing.
    #[test]
    fn a3_separator_variants_all_match() {
        for key in ["user-id", "User.Id", "USER_ID"] {
            let payload = json!({ key: "u1" });
            let o = obj(&payload);
            match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
                AliasMatch::Unique(k) => assert_eq!(k, key),
                other => panic!("separator variant {key} did not match: {other:?}"),
            }
        }
    }

    // A4 — negative control: abbreviations must NOT match. If this starts
    // failing, the matcher has become too permissive (substring/distance
    // matching instead of exact-after-normalize equality).
    #[test]
    fn a4_abbreviation_does_not_match() {
        for key in ["uid", "usr_id"] {
            let payload = json!({ key: "u1" });
            let o = obj(&payload);
            match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
                AliasMatch::None => {}
                other => panic!("abbreviation {key} incorrectly matched: {other:?}"),
            }
        }
    }

    // B1 — two candidates, same value: safe to resolve, and deterministic
    // (serde_json's default non-preserve_order Map iterates in sorted key
    // order, so the alphabetically-first candidate always wins).
    #[test]
    fn b1_multiple_candidates_same_value_resolves_unique() {
        let payload = json!({"user_id": "user-42", "userID": "user-42"});
        let o = obj(&payload);
        match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
            AliasMatch::Unique(k) => assert_eq!(k, "userID"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    // B2 — two candidates, conflicting values: must refuse to guess rather
    // than silently writing whichever one sorts first.
    #[test]
    fn b2_multiple_candidates_conflicting_values_ambiguous() {
        let payload = json!({"user_id": "user-42", "userID": "user-99"});
        let o = obj(&payload);
        match find_alias_candidate("userId", Some(&o), &HashSet::new()) {
            AliasMatch::Ambiguous(mut candidates) => {
                candidates.sort();
                assert_eq!(candidates, vec!["userID".to_string(), "user_id".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    // B3 — canonical name must win over a stale alias-shaped decoy with a
    // conflicting value (candidates list always tries canonical first).
    #[test]
    fn b3_canonical_key_takes_precedence_over_decoy_alias() {
        let mut aliases = default_aliases();
        aliases.get_mut("userId").unwrap().push("user_id".to_string());
        let payload = json!({
            "eventId": "e1", "userId": "user-42", "user_id": "STALE",
            "trackId": "t1", "timestamp": "2026-01-01T00:00:00"
        });
        let extracted = extract(&payload, &aliases);
        assert_eq!(extracted.values.get("userId"), Some(&"user-42".to_string()));
        assert_eq!(extracted.resolved_via.get("userId"), Some(&"userId".to_string()));
    }

    // C1 — two independent fields renamed simultaneously, unambiguous.
    #[test]
    fn c1_two_fields_renamed_simultaneously() {
        let payload = json!({
            "eventId": "e1", "user_id": "u1", "track_id": "t1",
            "timestamp": "2026-01-01T00:00:00"
        });
        let extracted = extract(&payload, &default_aliases());
        let mut missing = extracted.missing_required.clone();
        missing.sort();
        assert_eq!(missing, vec!["trackId".to_string(), "userId".to_string()]);

        let o = payload.as_object();
        let mut used = extracted.used_raw_keys.clone();
        for field in &extracted.missing_required {
            match find_alias_candidate(field, o, &used) {
                AliasMatch::Unique(k) => {
                    used.insert(k);
                }
                other => panic!("expected Unique for {field}, got {other:?}"),
            }
        }
    }

    // C2 — total drift: all four required fields renamed at once. Also
    // documents a real limit of the matcher: "ts" does not fuzzy-match
    // "timestamp" (normalize("ts") = "ts" != "timestamp") — an abbreviation
    // isn't the same thing as a case/separator variant.
    #[test]
    fn c2_total_drift_all_required_fields_renamed() {
        let payload = json!({
            "event_id": "e1", "user_id": "u1", "track_id": "t1", "ts": "2026-01-01T00:00:00"
        });
        let extracted = extract(&payload, &default_aliases());
        let mut missing = extracted.missing_required.clone();
        missing.sort();
        assert_eq!(missing, vec!["eventId", "timestamp", "trackId", "userId"]);

        let o = payload.as_object();
        assert!(matches!(
            find_alias_candidate("timestamp", o, &HashSet::new()),
            AliasMatch::None
        ));
        assert!(matches!(
            find_alias_candidate("eventId", o, &HashSet::new()),
            AliasMatch::Unique(ref k) if k == "event_id"
        ));
        assert!(matches!(
            find_alias_candidate("userId", o, &HashSet::new()),
            AliasMatch::Unique(ref k) if k == "user_id"
        ));
        assert!(matches!(
            find_alias_candidate("trackId", o, &HashSet::new()),
            AliasMatch::Unique(ref k) if k == "track_id"
        ));
    }

    // C3 — mixed repairable + unrepairable field in the SAME event: one
    // field should resolve to a fuzzy match, the other genuinely has no
    // candidate at all.
    #[test]
    fn c3_mixed_repairable_and_unrepairable_in_one_event() {
        let payload = json!({
            "eventId": "e1", "user_id": "u1", "timestamp": "2026-01-01T00:00:00"
        });
        let extracted = extract(&payload, &default_aliases());
        let mut missing = extracted.missing_required.clone();
        missing.sort();
        assert_eq!(missing, vec!["trackId".to_string(), "userId".to_string()]);

        let o = payload.as_object();
        let used = extracted.used_raw_keys.clone();
        assert!(matches!(
            find_alias_candidate("userId", o, &used),
            AliasMatch::Unique(ref k) if k == "user_id"
        ));
        assert!(matches!(find_alias_candidate("trackId", o, &used), AliasMatch::None));
    }

    // F1 — duplicate JSON keys are technically grammar-valid; pin down that
    // serde_json resolves them last-value-wins rather than erroring, since
    // that's an assumption the rest of the pipeline implicitly relies on.
    #[test]
    fn f1_duplicate_json_keys_last_value_wins() {
        let raw = r#"{"userId": "first", "userId": "second"}"#;
        let payload: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(payload["userId"], "second");
    }

    // F2 regression — within one repair pass, a key already claimed for one
    // field must not be reoffered to a second field whose normalized name
    // happens to collide. Can't be constructed with the real 5-field
    // ontology (no two canonical names collide today), so this uses
    // contrived field names to prove the *mechanism* validate.rs relies on
    // (threading `used` across loop iterations) actually holds, ahead of
    // whenever a 6th field makes the collision reachable for real.
    #[test]
    fn f2_used_keys_threaded_across_fields_in_one_pass() {
        let payload = json!({"fooid": "shared-value"});
        let o = payload.as_object();
        let mut used = HashSet::new();

        let key1 = match find_alias_candidate("fooId", o, &used) {
            AliasMatch::Unique(k) => k,
            other => panic!("expected Unique, got {other:?}"),
        };
        used.insert(key1);

        let second = find_alias_candidate("foo_Id", o, &used);
        assert!(
            matches!(second, AliasMatch::None),
            "second field must not reclaim an already-used key: {second:?}"
        );
    }
}
