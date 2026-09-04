//! Interactive console session wrapping WebSocket stream.

use crate::error::ClientError;
use dart_protocol::console::{ConsoleWsClientMessage, ConsoleWsServerMessage};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// Handle to an active interactive console session.
pub struct ConsoleSession<S> {
    inner: WebSocketStream<S>,
}

impl<S> ConsoleSession<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    /// Constructs a console session from an active WebSocket stream.
    pub fn new(stream: WebSocketStream<S>) -> Self {
        Self { inner: stream }
    }

    /// Dispatches a command to the instance's console stdin.
    pub async fn send_command(&mut self, command: impl Into<String>) -> Result<(), ClientError> {
        let msg = ConsoleWsClientMessage::Command {
            command: command.into(),
        };
        let json = serde_json::to_string(&msg)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;
        self.inner
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| ClientError::WebSocket(e.to_string()))
    }

    /// Sends a ping message to keep the connection alive.
    pub async fn ping(&mut self) -> Result<(), ClientError> {
        let msg = ConsoleWsClientMessage::Ping;
        let json = serde_json::to_string(&msg)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;
        self.inner
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| ClientError::WebSocket(e.to_string()))
    }

    /// Receives the next server message (history, live output line, or state change).
    pub async fn recv(&mut self) -> Result<Option<ConsoleWsServerMessage>, ClientError> {
        while let Some(msg_res) = self.inner.next().await {
            let msg = msg_res.map_err(|e| ClientError::WebSocket(e.to_string()))?;
            match msg {
                Message::Text(text) => {
                    let parsed: ConsoleWsServerMessage = serde_json::from_str(&text)
                        .map_err(|e| ClientError::Serialization(e.to_string()))?;
                    return Ok(Some(parsed));
                }
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }
}
