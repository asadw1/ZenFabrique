use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub ingestion: IngestionConfig,
    pub control_plane: ControlPlaneConfig,
    pub data_plane: DataPlaneConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize)]
pub struct IngestionConfig {
    #[serde(default)]
    pub backend: IngestionBackend,
    pub watch_dir: Option<PathBuf>,
    pub rabbitmq: Option<RabbitMqConfig>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum IngestionBackend {
    // The safe zero-infra default if `backend` is omitted entirely —
    // `config/fabric.yaml` itself opts into `rabbitmq` now that Phase 4 is
    // live, but a config that predates this field shouldn't suddenly
    // require a broker to start.
    #[default]
    FileWatch,
    Rabbitmq,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RabbitMqConfig {
    pub url: String,
    pub queue: String,
}

#[derive(Debug, Deserialize)]
pub struct ControlPlaneConfig {
    pub fuseki_url: String,
    pub shapes_path: PathBuf,
    #[serde(default = "default_username")]
    pub username: String,
    #[serde(default = "default_password")]
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct DataPlaneConfig {
    pub duckdb_path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { level: default_log_level() }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_username() -> String {
    "admin".to_string()
}

fn default_password() -> String {
    "admin".to_string()
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config file {}: {e}", path.display()))?;
        Ok(serde_yaml::from_str(&raw)?)
    }
}
