//! Console I/O, log queries, and WebSocket interactive terminal handlers.

use crate::error::ServerError;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use dart_daemon::{InstanceId, ServerEvent};
use dart_protocol::console::{
    ConsoleCommandRequest, ConsoleLineDto, ConsoleWsClientMessage, ConsoleWsServerMessage,
};
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub tail: Option<usize>,
}

/// Handler for `POST /api/v1/instances/:id/command`.
pub async fn send_command(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Json(request): Json<ConsoleCommandRequest>,
) -> Result<StatusCode, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    state
        .daemon()
        .send_console(&id, &request.command)
        .await
        .map_err(ServerError::from)?;
    Ok(StatusCode::OK)
}

/// Handler for `GET /api/v1/instances/:id/logs`.
pub async fn get_logs(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<Vec<ConsoleLineDto>>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let records = state.daemon().recent_logs(&id, query.tail);
    let dtos = records.into_iter().map(|rec| rec.to_dto(&id)).collect();
    Ok(Json(dtos))
}

/// Handler for `GET /api/v1/instances/:id/console` (WebSocket upgrade).
pub async fn ws_console(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<impl IntoResponse, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    // Verify instance exists
    let _ = state.daemon().get_instance(&id).map_err(ServerError::from)?;
    Ok(ws.on_upgrade(move |socket| handle_console_socket(socket, state, id)))
}

async fn handle_console_socket(mut socket: WebSocket, state: AppState, id: InstanceId) {
    // Replay recent buffered history immediately
    let history = state.daemon().recent_logs(&id, None);
    let history_lines = history.into_iter().map(|rec| rec.to_dto(&id)).collect();
    let initial = ConsoleWsServerMessage::History {
        lines: history_lines,
    };
    if let Ok(json) = serde_json::to_string(&initial) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    let mut events = state.daemon().subscribe_events();

    loop {
        tokio::select! {
            event_res = events.recv() => {
                match event_res {
                    Ok(ServerEvent::ConsoleLine { id: event_id, stream, line }) if event_id == id => {
                        let timestamp_millis = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                        let msg = ConsoleWsServerMessage::Output(ConsoleLineDto {
                            id: id.to_string(),
                            stream: stream.into(),
                            line,
                            timestamp_millis,
                        });
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(ServerEvent::StateChanged { id: event_id, state: new_state }) if event_id == id => {
                        let msg = ConsoleWsServerMessage::StateChanged {
                            state: (&new_state).into(),
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(client_msg) = serde_json::from_str::<ConsoleWsClientMessage>(&text) {
                            match client_msg {
                                ConsoleWsClientMessage::Command { command } => {
                                    let _ = state.daemon().send_console(&id, &command).await;
                                }
                                ConsoleWsClientMessage::Ping => {
                                    let pong = ConsoleWsServerMessage::Pong;
                                    if let Ok(json) = serde_json::to_string(&pong) {
                                        let _ = socket.send(Message::Text(json.into())).await;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
