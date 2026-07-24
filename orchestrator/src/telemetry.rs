use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

// A live tail of the orchestrator's own tracing output, pushed to any
// connected Control Room clients over a WebSocket — the "Observability
// Stream" from README.md. Deliberately built as a tracing_subscriber Layer
// rather than threading a broadcast call through every call site (opa.rs,
// shacl.rs, fhe.rs, validate.rs, ...): every decision point already emits a
// structured tracing event, so intercepting those directly means the
// console mirrors exactly what operators see in the terminal today, with
// zero changes to business logic.
const CHANNEL_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct TelemetryHub {
    sender: broadcast::Sender<String>,
}

impl TelemetryHub {
    pub fn new() -> Self {
        let (sender, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    pub fn layer(&self) -> BroadcastLayer {
        BroadcastLayer {
            sender: self.sender.clone(),
        }
    }

    // Runs the WS server on a dedicated background thread with its own
    // tokio runtime — the same pattern amqp.rs uses, so the rest of the
    // orchestrator stays synchronous rather than converting wholesale to
    // async for the sake of one component.
    pub fn serve(&self, addr: &str) {
        let addr = addr.to_string();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    // Not `tracing::error!` — see BroadcastLayer::on_event's
                    // comment on why this module never logs through tracing.
                    eprintln!("failed to start tokio runtime for telemetry WS server: {e}");
                    return;
                }
            };
            if let Err(e) = runtime.block_on(accept_loop(&addr, sender)) {
                eprintln!("telemetry WS server terminated with error: {e}");
            }
        });
    }

    #[cfg(test)]
    pub fn sender(&self) -> broadcast::Sender<String> {
        self.sender.clone()
    }
}

async fn accept_loop(addr: &str, sender: broadcast::Sender<String>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _peer) = listener.accept().await?;
        let mut rx = sender.subscribe();
        tokio::spawn(async move {
            let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let (mut write, mut read) = ws_stream.split();
            loop {
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Ok(line) => {
                                if write.send(Message::Text(line.into())).await.is_err() {
                                    break;
                                }
                            }
                            // A slow client that falls behind the ring buffer
                            // just misses old lines, same as tailing a log
                            // file that's rotating fast — not worth dropping
                            // the connection over.
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    incoming = read.next() => {
                        // Clients only ever receive; anything they send
                        // (including a close frame) just ends this task.
                        if incoming.is_none() {
                            break;
                        }
                        if matches!(incoming, Some(Err(_))) {
                            break;
                        }
                    }
                }
            }
        });
    }
}

pub struct BroadcastLayer {
    sender: broadcast::Sender<String>,
}

#[derive(Default)]
struct JsonVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let text = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.fields.insert(field.name().to_string(), Value::String(text));
        }
    }
}

impl<S: Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);

        let line = serde_json::json!({
            "ts_ms": now_ms(),
            "level": event.metadata().level().to_string(),
            "target": event.metadata().target(),
            "message": visitor.message.unwrap_or_default(),
            "fields": visitor.fields,
        })
        .to_string();

        // Fire-and-forget: `send` errors only when there are zero
        // subscribers (no UI connected right now), which is never worth
        // failing over — and must never be reported via `tracing::*`, since
        // that would re-enter this exact function.
        let _ = self.sender.send(line);
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    // A dedicated non-global subscriber (tracing::subscriber::with_default)
    // rather than `.init()`'s process-global one, so parallel test threads
    // don't fight over the same tracing dispatcher.
    #[test]
    fn tracing_event_is_broadcast_as_structured_json() {
        let hub = TelemetryHub::new();
        let mut rx = hub.sender().subscribe();
        let subscriber = tracing_subscriber::registry().with(hub.layer());

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(source = "test.json", missing = "userId", "schema breach detected");
        });

        let line = rx.try_recv().expect("expected one broadcast message");
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["level"], "WARN");
        assert_eq!(parsed["message"], "schema breach detected");
        assert_eq!(parsed["fields"]["source"], "\"test.json\"");
        assert_eq!(parsed["fields"]["missing"], "\"userId\"");
    }

    #[test]
    fn send_with_no_subscribers_does_not_panic_or_block() {
        let hub = TelemetryHub::new();
        let subscriber = tracing_subscriber::registry().with(hub.layer());

        // No `.subscribe()` call at all — this must not panic, block, or
        // require a receiver to exist, since most of the time nothing is
        // connected to the console.
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("nobody is listening");
        });
    }

    #[tokio::test]
    async fn live_ws_client_receives_broadcast_events() {
        let hub = TelemetryHub::new();
        hub.serve("127.0.0.1:19081");
        // Give the background thread's runtime a moment to bind the
        // listener before connecting.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let (ws_stream, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:19081")
            .await
            .expect("failed to connect to telemetry WS server");
        let (_, mut read) = ws_stream.split();

        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(source = "e1.json", "schema breach detected — attempting self-healing repair");
        });

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), read.next())
            .await
            .expect("timed out waiting for telemetry message")
            .expect("stream ended unexpectedly")
            .expect("websocket error");

        let text = msg.into_text().unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["level"], "WARN");
        assert_eq!(parsed["message"], "schema breach detected — attempting self-healing repair");
        assert_eq!(parsed["fields"]["source"], "\"e1.json\"");
    }
}
