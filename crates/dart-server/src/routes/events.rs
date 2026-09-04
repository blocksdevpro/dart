//! Global daemon events WebSocket endpoint.

use crate::error::ServerError;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use dart_daemon::ServerEvent;
use dart_protocol::event::DaemonEvent;

/// Handler for `GET /api/v1/events` (WebSocket upgrade).
pub async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ServerError> {
    Ok(ws.on_upgrade(move |socket| handle_events_socket(socket, state)))
}

async fn handle_events_socket(mut socket: WebSocket, state: AppState) {
    let mut events = state.daemon().subscribe_events();

    loop {
        tokio::select! {
            event_res = events.recv() => {
                match event_res {
                    Ok(ServerEvent::StateChanged { id, state }) => {
                        let daemon_event = DaemonEvent::StateChanged {
                            id: id.to_string(),
                            state: (&state).into(),
                        };
                        if let Ok(json) = serde_json::to_string(&daemon_event) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(ServerEvent::ConsoleLine { id, stream, line }) => {
                        let timestamp_millis = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                        let daemon_event = DaemonEvent::ConsoleLine {
                            id: id.to_string(),
                            stream: stream.into(),
                            line,
                            timestamp_millis,
                        };
                        if let Ok(json) = serde_json::to_string(&daemon_event) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(ServerEvent::OperationFailed { id, message }) => {
                        let daemon_event = DaemonEvent::OperationFailed {
                            id: id.to_string(),
                            message,
                        };
                        if let Ok(json) = serde_json::to_string(&daemon_event) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
