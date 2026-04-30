//! Shim-side event bus. Every `emit_*` helper in `crate::events::emitter`
//! calls [`broadcast`] in addition to its existing `app.emit` so the shim's
//! WebSocket handler can forward events to browser clients.
//!
//! The bus is a [`tokio::sync::broadcast`] channel wrapped in a [`OnceLock`].
//! When the shim is not running (e.g. during tests, or if the server fails to
//! bind) the bus is uninitialised and [`broadcast`] silently drops. This keeps
//! the desktop path zero-cost when nothing is listening.

use std::sync::OnceLock;

use axum::extract::ws::{Message, WebSocket};
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::debug;

/// A serialised event ready to ship over the WebSocket.
#[derive(Clone, Debug, Serialize)]
pub struct EventEnvelope {
    pub channel: &'static str,
    pub payload: serde_json::Value,
}

static BUS: OnceLock<broadcast::Sender<EventEnvelope>> = OnceLock::new();

/// Initialise the broadcast bus. Called once by the shim server during
/// startup. Capacity is intentionally generous so log-chunk streams don't
/// induce lag on other event types; slow subscribers will still see
/// `RecvError::Lagged` which the WS handler surfaces to the client.
pub fn init_bus() -> broadcast::Sender<EventEnvelope> {
    let (tx, _) = broadcast::channel(1024);
    // `set` only fails if already initialised — harmless; we keep the original.
    let _ = BUS.set(tx.clone());
    BUS.get().cloned().unwrap_or(tx)
}

/// Returns a fresh receiver if the bus is initialised.
#[allow(dead_code)]
pub fn subscribe() -> Option<broadcast::Receiver<EventEnvelope>> {
    BUS.get().map(|tx| tx.subscribe())
}

/// Push an event into the bus. No-op when the bus has not been initialised.
/// Failure to send (no subscribers) is expected and silently ignored.
pub fn broadcast<T: Serialize>(channel: &'static str, payload: &T) {
    let Some(tx) = BUS.get() else { return };
    let value = match serde_json::to_value(payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("shim::ws::broadcast: serialize {}: {}", channel, e);
            return;
        }
    };
    let _ = tx.send(EventEnvelope {
        channel,
        payload: value,
    });
}

/// WebSocket per-connection task. Owns a fresh subscription to the event bus
/// and forwards each envelope as a JSON text frame to the browser.
///
/// The frontend receives one message shape: `{"channel": "...", "payload": …}`.
/// If the consumer falls behind the broadcast channel capacity, we send a
/// `{"type":"lagged","missed":N}` notice so the UI can warn the user rather
/// than silently dropping state.
pub async fn handle_socket(mut socket: WebSocket) {
    let Some(mut rx) = subscribe() else {
        // Bus not initialised: close immediately with a reason.
        let _ = socket
            .send(Message::Text(
                serde_json::json!({ "type": "error", "error": "event bus not initialised" })
                    .to_string(),
            ))
            .await;
        return;
    };

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        debug!("shim ws recv error: {}", e);
                        break;
                    }
                    // Ignore text/binary/ping from the client for now; protocol
                    // is server-push only. Future phases may add per-channel
                    // subscription filters.
                    Some(Ok(_)) => {}
                }
            }
            next = rx.recv() => match next {
                Ok(env) => {
                    let text = match serde_json::to_string(&env) {
                        Ok(s) => s,
                        Err(e) => {
                            debug!("shim ws serialize error: {}", e);
                            continue;
                        }
                    };
                    if socket.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    let notice = serde_json::json!({ "type": "lagged", "missed": missed }).to_string();
                    if socket.send(Message::Text(notice)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}
