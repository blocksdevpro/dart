//! System and instance event models for SSE and WebSocket event feeds.

use crate::console::OutputStreamDto;
use crate::instance::InstanceStateDto;
use serde::{Deserialize, Serialize};

/// Broadcast events emitted by the daemon.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum DaemonEvent {
    /// An instance process transitioned to a new lifecycle state.
    StateChanged {
        /// The instance whose state changed.
        id: String,
        /// New lifecycle state.
        state: InstanceStateDto,
    },
    /// A line of console output was produced.
    ConsoleLine {
        /// The instance ID.
        id: String,
        /// The stream (`stdout` or `stderr`).
        stream: OutputStreamDto,
        /// Raw output line.
        line: String,
        /// Timestamp in milliseconds since Unix epoch.
        timestamp_millis: u64,
    },
    /// An operation on an instance failed.
    OperationFailed {
        /// Target instance ID.
        id: String,
        /// Failure message.
        message: String,
    },
    /// A new instance was created.
    InstanceCreated {
        /// New instance ID.
        id: String,
    },
    /// An instance was deleted.
    InstanceDeleted {
        /// Deleted instance ID.
        id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_daemon_events() {
        let event = DaemonEvent::StateChanged {
            id: "survival".to_owned(),
            state: InstanceStateDto::Running { pid: 1234 },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"state_changed""#));
        assert!(json.contains(r#""id":"survival""#));

        let decoded: DaemonEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, decoded);
    }
}
