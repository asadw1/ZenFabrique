use crate::ingest::RawEvent;
use crate::rdf;
use crate::shacl::ShaclClient;
use crate::shim::ShimEngine;
use anyhow::Result;
use tracing::{info, warn};

// The Observe -> Reason -> Act loop for a single event:
//   1. extract known fields, build RDF, validate against the SHACL contract
//   2. on breach, try to fuzzy-match missing fields against the event's
//      actual keys and widen the shim's alias registry (the "repair")
//   3. re-validate to confirm the repair actually worked
//   4. always persist the raw event so the shim view can (retroactively)
//      reflect it once/if it's healed
pub fn process(event: &RawEvent, shacl: &ShaclClient, shim: &mut ShimEngine) -> Result<()> {
    let source_path = event.source_path.display().to_string();
    let payload_text = event.payload.to_string();
    let fallback_id = event
        .source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut extracted = rdf::extract(&event.payload, shim.aliases());

    if !extracted.missing_required.is_empty() {
        warn!(
            path = %source_path,
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
                    info!(
                        canonical = %field,
                        raw_key = %raw_key,
                        path = %source_path,
                        "self-healing: discovered renamed field, widening shim"
                    );
                    used.insert(raw_key.clone());
                    if shim.learn_alias(&field, &raw_key)? {
                        learned_any = true;
                    }
                }
                rdf::AliasMatch::Ambiguous(candidates) => {
                    warn!(
                        canonical = %field,
                        candidates = ?candidates,
                        path = %source_path,
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

    let turtle = rdf::build_turtle(&fallback_id, &extracted);

    let conforms = if extracted.missing_required.is_empty() {
        match shacl.validate(&turtle) {
            Ok(conforms) => conforms,
            Err(e) => {
                warn!(path = %source_path, error = %e, "SHACL validation call failed");
                false
            }
        }
    } else {
        false
    };

    if conforms {
        info!(path = %source_path, "event conforms to StreamingEvent contract");
    } else {
        warn!(
            path = %source_path,
            missing = ?extracted.missing_required,
            "event does not conform — stored for audit, not healed"
        );
    }

    shim.insert_raw(&source_path, &payload_text)?;

    Ok(())
}
