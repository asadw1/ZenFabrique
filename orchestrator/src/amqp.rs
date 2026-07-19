use crate::ingest::RawEvent;
use anyhow::{Context, Result};
use futures_lite::stream::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties};
use std::sync::mpsc::{self, Receiver};
use tracing::{info, warn};

// The Phase 4 transport swap: same `Receiver<RawEvent>` shape as
// `ingest::watch`, so `main.rs` picks one or the other based on config and
// nothing downstream (validate/shim) needs to know which is active. Runs
// its own dedicated tokio runtime on a background thread rather than
// making the whole orchestrator async — everything else stays synchronous.
pub fn watch(url: &str, queue: &str) -> Result<Receiver<RawEvent>> {
    let (event_tx, event_rx) = mpsc::channel::<RawEvent>();
    let url = url.to_string();
    let queue = queue.to_string();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(error = %e, "failed to start tokio runtime for RabbitMQ consumer");
                return;
            }
        };

        if let Err(e) = runtime.block_on(consume(&url, &queue, event_tx)) {
            tracing::error!(error = %e, "RabbitMQ consumer terminated with error");
        }
    });

    Ok(event_rx)
}

async fn consume(url: &str, queue: &str, event_tx: mpsc::Sender<RawEvent>) -> Result<()> {
    let conn = Connection::connect(url, ConnectionProperties::default())
        .await
        .with_context(|| format!("failed to connect to RabbitMQ at {url}"))?;
    let channel = conn
        .create_channel()
        .await
        .context("failed to create AMQP channel")?;

    channel
        .queue_declare(queue.into(), QueueDeclareOptions::durable(), FieldTable::default())
        .await
        .with_context(|| format!("failed to declare queue {queue}"))?;

    let mut consumer = channel
        .basic_consume(
            queue.into(),
            "zenfabrique-orchestrator".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .with_context(|| format!("failed to start consuming from {queue}"))?;

    info!(queue = %queue, "listening for events on RabbitMQ");

    while let Some(delivery) = consumer.next().await {
        let delivery = match delivery {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "RabbitMQ delivery error");
                continue;
            }
        };

        let delivery_tag = delivery.delivery_tag;

        match serde_json::from_slice::<serde_json::Value>(&delivery.data) {
            Ok(payload) => {
                let event = RawEvent {
                    source: format!("amqp:{queue}:{delivery_tag}"),
                    fallback_id: format!("amqp-{delivery_tag}"),
                    payload,
                };
                if event_tx.send(event).is_err() {
                    // receiver dropped (orchestrator shutting down) — ack
                    // what we already dequeued and stop consuming.
                    let _ = delivery.ack(BasicAckOptions::default()).await;
                    break;
                }
            }
            Err(e) => {
                warn!(delivery_tag, error = %e, "failed to parse JSON from RabbitMQ message");
            }
        }

        // Ack regardless of parse outcome — a poison message would
        // otherwise redeliver forever. Mirrors file-watch's behavior, where
        // a malformed event is logged and skipped rather than retried.
        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
            warn!(delivery_tag, error = %e, "failed to ack RabbitMQ delivery");
        }
    }

    Ok(())
}
