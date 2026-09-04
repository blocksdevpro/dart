//! Console I/O, log streaming, and interactive terminal protocols.

use crate::instance::InstanceStateDto;
use serde::{Deserialize, Serialize};

/// Target stream for console text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStreamDto {
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
}

/// A line of console output emitted by an instance process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConsoleLineDto {
    /// Target instance ID.
    pub id: String,
    /// Stream origin (`stdout` or `stderr`).
    pub stream: OutputStreamDto,
    /// Raw line text.
    pub line: String,
    /// Milliseconds since Unix epoch when the line was captured.
    pub timestamp_millis: u64,
}

/// Request to dispatch a console command to a running instance's standard input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConsoleCommandRequest {
    /// Command line text to execute (without trailing newline).
    pub command: String,
}

/// Incoming WebSocket messages from a client on `/api/v1/instances/:id/console`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsoleWsClientMessage {
    /// Send a command to the server's standard input.
    Command {
        /// Command text to write.
        command: String,
    },
    /// Keep-alive ping.
    Ping,
}

/// Outgoing WebSocket messages sent by the daemon to attached console clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsoleWsServerMessage {
    /// Replay of recent log lines sent immediately upon connection.
    History {
        /// Buffered log lines from the ring buffer.
        lines: Vec<ConsoleLineDto>,
    },
    /// Live console line output.
    Output(ConsoleLineDto),
    /// Notification that the instance lifecycle state changed.
    StateChanged {
        /// Current lifecycle state.
        state: InstanceStateDto,
    },
    /// Keep-alive pong response.
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_console_ws_messages() {
        let msg = ConsoleWsClientMessage::Command {
            command: "list".to_owned(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"command","command":"list"}"#);

        let srv_msg = ConsoleWsServerMessage::Output(ConsoleLineDto {
            id: "survival".to_owned(),
            stream: OutputStreamDto::Stdout,
            line: "Server started".to_owned(),
            timestamp_millis: 1756890000000,
        });
        let srv_json = serde_json::to_string(&srv_msg).unwrap();
        assert!(srv_json.contains(r#""type":"output""#));
        assert!(srv_json.contains(r#""stream":"stdout""#));
    }
}
