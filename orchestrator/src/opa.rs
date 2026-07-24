use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

// Zero-Trust gate in front of the shim's alias-learning: a repair widens
// how every future event with that raw key is interpreted, so it's treated
// as a schema mutation and checked against policy rather than applied just
// because the fuzzy-matcher found a unique candidate.
pub enum OpaClient {
    Remote { decision_url: String },
    // No OPA deployed (policy_plane omitted from config) — schema mutations
    // go unrestricted rather than the orchestrator refusing to start, so
    // local dev doesn't require standing up OPA to see the self-healing
    // loop work.
    Disabled,
}

#[derive(Deserialize)]
struct DecisionResponse {
    result: Option<bool>,
}

impl OpaClient {
    pub fn remote(opa_url: &str) -> Self {
        Self::Remote {
            decision_url: format!(
                "{}/v1/data/zenfabrique/schema_mutation/allow",
                opa_url.trim_end_matches('/')
            ),
        }
    }

    pub fn disabled() -> Self {
        Self::Disabled
    }

    // Fail-closed: if OPA is unreachable, the decision path errors, or the
    // rule path returns no result (e.g. the policy failed to load), the
    // mutation is denied. A security gate that fails open on outage would
    // silently defeat the point of having it — unlike SHACL validation,
    // where "can't reach Fuseki" just means "don't heal this one event yet,"
    // an unreachable policy engine must not become "mutations are
    // unrestricted."
    pub fn allow_mutation(&self, source: &str, field: &str, raw_key: &str) -> bool {
        match self {
            OpaClient::Disabled => true,
            OpaClient::Remote { decision_url } => {
                match query(decision_url, source, field, raw_key) {
                    Ok(allow) => allow,
                    Err(e) => {
                        tracing::warn!(
                            source = %source,
                            field = %field,
                            error = %e,
                            "policy check unreachable — refusing schema mutation (fail-closed)"
                        );
                        false
                    }
                }
            }
        }
    }
}

fn query(decision_url: &str, source: &str, field: &str, raw_key: &str) -> Result<bool> {
    let body = json!({
        "input": {
            "source": source,
            "field": field,
            "raw_key": raw_key,
        }
    });

    let response_text = ureq::post(decision_url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .context("failed to call OPA decision endpoint")?
        .into_string()
        .context("failed to read OPA decision response")?;

    let response: DecisionResponse = serde_json::from_str(&response_text)
        .context("failed to parse OPA decision response")?;

    Ok(response.result.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_client_always_allows() {
        let opa = OpaClient::disabled();
        assert!(opa.allow_mutation("any-source", "userId", "user_id"));
        assert!(opa.allow_mutation("any-source", "trackId", "track_id"));
    }

    // Port 1 has nothing listening, so connections fail fast (refused)
    // rather than hanging on a timeout — mirrors shacl.rs's dead_shacl_client
    // pattern for exercising the unreachable-policy-engine path without
    // needing to actually stop a docker container from a test.
    #[test]
    fn unreachable_opa_denies_fail_closed() {
        let opa = OpaClient::remote("http://127.0.0.1:1");
        assert!(!opa.allow_mutation("partner-feed", "userId", "user_id"));
        assert!(!opa.allow_mutation("partner-feed", "trackId", "track_id"));
    }

    // These require `docker compose up -d opa` with schema_mutation.rego
    // loaded (the compose service mounts policy-plane/rego/ directly, so
    // no manual load step is needed — just a running container).
    #[test]
    #[ignore = "requires live OPA at localhost:8181 with policy-plane/rego loaded"]
    fn unprotected_field_allowed_for_any_source() {
        let opa = OpaClient::remote("http://localhost:8181");
        assert!(opa.allow_mutation("untrusted-source", "trackId", "track_id"));
    }

    #[test]
    #[ignore = "requires live OPA at localhost:8181 with policy-plane/rego loaded"]
    fn protected_field_denied_for_untrusted_source() {
        let opa = OpaClient::remote("http://localhost:8181");
        assert!(!opa.allow_mutation("untrusted-source", "userId", "user_id"));
    }

    #[test]
    #[ignore = "requires live OPA at localhost:8181 with policy-plane/rego loaded"]
    fn protected_field_allowed_for_trusted_source() {
        let opa = OpaClient::remote("http://localhost:8181");
        assert!(opa.allow_mutation("partner-feed", "userId", "user_id"));
    }
}
