mod config;
mod ingest;
mod rdf;
mod shacl;
mod shim;
mod validate;

use clap::Parser;
use config::Config;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "zenfabrique-orchestrator")]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.logging.level.clone()))
        .init();

    tracing::info!(
        watch_dir = %config.ingestion.watch_dir.display(),
        "starting ZenFabrique orchestrator"
    );

    let shapes = std::fs::read_to_string(&config.control_plane.shapes_path)?;
    let shacl_client = shacl::ShaclClient::new(
        config.control_plane.fuseki_url.clone(),
        shapes,
        &config.control_plane.username,
        &config.control_plane.password,
    );

    let mut shim = shim::ShimEngine::open(&config.data_plane.duckdb_path, rdf::default_aliases())?;

    std::fs::create_dir_all(&config.ingestion.watch_dir)?;
    let (events, _watcher) = ingest::watch(&config.ingestion.watch_dir)?;

    for event in events {
        if let Err(e) = validate::process(&event, &shacl_client, &mut shim) {
            tracing::warn!(error = %e, "failed to process event");
        }
    }

    Ok(())
}
