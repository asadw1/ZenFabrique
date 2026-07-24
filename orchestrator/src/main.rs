mod amqp;
mod config;
mod fhe;
mod ingest;
mod opa;
mod rdf;
mod shacl;
mod shim;
mod validate;

use clap::Parser;
use config::{Config, IngestionBackend};
use notify::RecommendedWatcher;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "zenfabrique-orchestrator")]
struct Args {
    #[arg(long)]
    config: PathBuf,
    // One-shot mode: print a user's total msPlayed (summed homomorphically
    // over their FHE-encrypted events, decrypted only for this final
    // aggregate) instead of running the ingestion loop.
    #[arg(long)]
    aggregate_user: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.logging.level.clone()))
        .init();

    let fhe_client = match &config.privacy_plane {
        Some(privacy) => fhe::FheClient::remote(&privacy.fhe_url),
        None => fhe::FheClient::disabled(),
    };

    if let Some(user_id) = &args.aggregate_user {
        let shim = shim::ShimEngine::open(&config.data_plane.duckdb_path, rdf::default_aliases())?;
        let ciphertexts = shim.ciphertexts_for_user(user_id)?;
        if ciphertexts.is_empty() {
            tracing::warn!(user_id = %user_id, "no encrypted metrics found for this user");
            return Ok(());
        }
        let total_ms_played = fhe_client.aggregate(&ciphertexts)?;
        println!("{user_id}: {total_ms_played} ms played (aggregated over {} events, decrypted only for this final total)", ciphertexts.len());
        return Ok(());
    }

    tracing::info!(backend = ?config.ingestion.backend, "starting ZenFabrique orchestrator");

    let shapes = std::fs::read_to_string(&config.control_plane.shapes_path)?;
    let shacl_client = shacl::ShaclClient::new(
        config.control_plane.fuseki_url.clone(),
        shapes,
        &config.control_plane.username,
        &config.control_plane.password,
    );

    let mut shim = shim::ShimEngine::open(&config.data_plane.duckdb_path, rdf::default_aliases())?;

    let opa_client = match &config.policy_plane {
        Some(policy) => {
            tracing::info!(opa_url = %policy.opa_url, "schema mutations gated by OPA policy");
            opa::OpaClient::remote(&policy.opa_url)
        }
        None => {
            tracing::warn!("no policy_plane configured — schema mutations are unrestricted");
            opa::OpaClient::disabled()
        }
    };

    match &config.privacy_plane {
        Some(privacy) => tracing::info!(fhe_url = %privacy.fhe_url, "msPlayed encrypted via FHE service at ingest"),
        None => tracing::warn!("no privacy_plane configured — msPlayed is not encrypted at rest"),
    }

    // Same `Receiver<RawEvent>` regardless of backend — this is the payoff
    // of keeping ingestion behind one channel: swapping transports touches
    // only this block, not validate::process or anything downstream.
    let (events, _watcher_guard): (Receiver<ingest::RawEvent>, Option<RecommendedWatcher>) =
        match config.ingestion.backend {
            IngestionBackend::FileWatch => {
                let watch_dir = config
                    .ingestion
                    .watch_dir
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("ingestion.watch_dir is required when backend = file_watch"))?;
                std::fs::create_dir_all(watch_dir)?;
                tracing::info!(watch_dir = %watch_dir.display(), "watching for events");
                let (rx, watcher) = ingest::watch(watch_dir)?;
                (rx, Some(watcher))
            }
            IngestionBackend::Rabbitmq => {
                let rmq = config.ingestion.rabbitmq.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("ingestion.rabbitmq is required when backend = rabbitmq")
                })?;
                tracing::info!(queue = %rmq.queue, "connecting to RabbitMQ");
                let rx = amqp::watch(&rmq.url, &rmq.queue)?;
                (rx, None)
            }
        };

    for event in events {
        if let Err(e) = validate::process(&event, &shacl_client, &opa_client, &fhe_client, &mut shim) {
            tracing::warn!(error = %e, "failed to process event");
        }
    }

    Ok(())
}
