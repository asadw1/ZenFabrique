use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

// Encrypts the numeric usage value (msPlayed) associated with a user via
// the Dockerized OpenFHE service (see privacy-plane/fhe/), so it's never
// stored in the clear anywhere but the raw JSON audit log. Aggregation
// happens homomorphically server-side — the service never decrypts an
// individual event's value, only a final requested aggregate (see
// docs/planning/STATUS.md for the accepted scope note on what this does
// and doesn't protect).
pub enum FheClient {
    Remote { base_url: String },
    // No FHE service deployed (privacy_plane omitted from config) —
    // msPlayed just isn't encrypted, so local dev doesn't require standing
    // up the FHE service to see the rest of the pipeline work.
    Disabled,
}

#[derive(Deserialize)]
struct EncryptResponse {
    ciphertext: String,
}

#[derive(Deserialize)]
struct AggregateResponse {
    sum: i64,
}

impl FheClient {
    pub fn remote(fhe_url: &str) -> Self {
        Self::Remote {
            base_url: fhe_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn disabled() -> Self {
        Self::Disabled
    }

    // A privacy feature, not a security gate — unlike OPA's fail-closed
    // policy check, a value that fails to encrypt just doesn't get an
    // encrypted-at-rest copy this time (logged by the caller); it never
    // blocks the event from being processed, since msPlayed is optional in
    // the SHACL contract to begin with.
    pub fn encrypt(&self, value: i64) -> Result<Option<String>> {
        match self {
            FheClient::Disabled => Ok(None),
            FheClient::Remote { base_url } => encrypt_remote(base_url, value).map(Some),
        }
    }

    // Sums a batch of ciphertexts homomorphically server-side and returns
    // only the decrypted final total — the FHE service never decrypts an
    // individual input to compute this.
    pub fn aggregate(&self, ciphertexts: &[String]) -> Result<i64> {
        match self {
            FheClient::Disabled => {
                anyhow::bail!("cannot aggregate: no FHE service configured (privacy_plane omitted)")
            }
            FheClient::Remote { base_url } => aggregate_remote(base_url, ciphertexts),
        }
    }
}

fn encrypt_remote(base_url: &str, value: i64) -> Result<String> {
    let url = format!("{base_url}/encrypt");
    let body = json!({ "value": value });

    let response_text = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .context("failed to call FHE service /encrypt endpoint")?
        .into_string()
        .context("failed to read FHE service /encrypt response")?;

    let response: EncryptResponse = serde_json::from_str(&response_text)
        .context("failed to parse FHE service /encrypt response")?;

    Ok(response.ciphertext)
}

fn aggregate_remote(base_url: &str, ciphertexts: &[String]) -> Result<i64> {
    let url = format!("{base_url}/aggregate");
    let body = json!({ "ciphertexts": ciphertexts });

    let response_text = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .context("failed to call FHE service /aggregate endpoint")?
        .into_string()
        .context("failed to read FHE service /aggregate response")?;

    let response: AggregateResponse = serde_json::from_str(&response_text)
        .context("failed to parse FHE service /aggregate response")?;

    Ok(response.sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_client_never_encrypts() {
        let fhe = FheClient::disabled();
        assert_eq!(fhe.encrypt(180_000).unwrap(), None);
    }

    // Port 1 has nothing listening, so connections fail fast (refused)
    // rather than hanging on a timeout — mirrors opa.rs/shacl.rs's dead-
    // client pattern for exercising the unreachable-service path.
    #[test]
    fn unreachable_fhe_service_errors() {
        let fhe = FheClient::remote("http://127.0.0.1:1");
        assert!(fhe.encrypt(180_000).is_err());
    }

    // Requires `docker compose up -d fhe`.
    #[test]
    #[ignore = "requires the live FHE service at localhost:8090"]
    fn live_encrypt_round_trips_through_service() {
        let fhe = FheClient::remote("http://localhost:8090");
        let ciphertext = fhe.encrypt(180_000).unwrap().expect("remote client must return Some");
        assert!(!ciphertext.is_empty());
    }

    #[test]
    #[ignore = "requires the live FHE service at localhost:8090"]
    fn live_aggregate_sums_without_decrypting_individually() {
        let fhe = FheClient::remote("http://localhost:8090");
        let values = [180_000i64, 210_000, 195_000];
        let ciphertexts: Vec<String> = values
            .iter()
            .map(|v| fhe.encrypt(*v).unwrap().unwrap())
            .collect();

        let sum = fhe.aggregate(&ciphertexts).unwrap();
        assert_eq!(sum, values.iter().sum::<i64>());
    }

    #[test]
    fn disabled_client_refuses_to_aggregate() {
        let fhe = FheClient::disabled();
        assert!(fhe.aggregate(&["anything".to_string()]).is_err());
    }
}
