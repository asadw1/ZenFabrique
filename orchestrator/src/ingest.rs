use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use tracing::{info, warn};

pub struct RawEvent {
    pub source_path: PathBuf,
    pub payload: serde_json::Value,
}

// Ingestion is intentionally file-watch-only for the vertical slice. Keeping
// it behind this single channel is what lets Phase 4 swap in a RabbitMQ
// consumer without touching validation/shim logic downstream (see
// ARCHITECTURE_DECISIONS.md).
pub fn watch(watch_dir: &Path) -> Result<(Receiver<RawEvent>, RecommendedWatcher)> {
    let (fs_tx, fs_rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher =
        notify::recommended_watcher(fs_tx).context("failed to create filesystem watcher")?;
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch {}", watch_dir.display()))?;

    let (event_tx, event_rx) = mpsc::channel::<RawEvent>();

    std::thread::spawn(move || {
        for res in fs_rx {
            let fs_event = match res {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "filesystem watch error");
                    continue;
                }
            };

            if !matches!(fs_event.kind, EventKind::Create(_)) {
                continue;
            }

            for path in fs_event.paths {
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                info!(path = %path.display(), "event file detected");

                match read_event(&path) {
                    Ok(payload) => {
                        if event_tx
                            .send(RawEvent { source_path: path, payload })
                            .is_err()
                        {
                            return; // receiver dropped, shut the watcher thread down
                        }
                    }
                    Err(e) => warn!(path = %path.display(), error = %e, "failed to read event file"),
                }
            }
        }
    });

    Ok((event_rx, watcher))
}

// notify's Create event can fire before the writer has flushed the file's
// contents (observed on Windows), so a fresh file may briefly read as empty
// or truncated. Retry with backoff instead of failing on the first read.
fn read_event(path: &Path) -> Result<serde_json::Value> {
    const MAX_ATTEMPTS: u32 = 5;
    let mut last_err = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50 * attempt as u64));
        }

        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) => {
                last_err = Some(anyhow::Error::new(e).context(format!("failed to read {}", path.display())));
                continue;
            }
        };

        if raw.trim().is_empty() {
            last_err = Some(anyhow::anyhow!("{} was empty on read (attempt {})", path.display(), attempt + 1));
            continue;
        }

        match serde_json::from_str(&raw) {
            Ok(value) => return Ok(value),
            Err(e) => {
                last_err = Some(anyhow::Error::new(e).context(format!("failed to parse JSON in {}", path.display())));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("failed to read {} after {} attempts", path.display(), MAX_ATTEMPTS)))
}