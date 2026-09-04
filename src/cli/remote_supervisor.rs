//! Transparent remote supervisor adapter delegating to `dartd` via `DartClient`.

use dart_client::DartClient;
use dart_daemon::{
    ConsoleLineRecord, InstanceId, InstanceState, OutputStream, ServerEvent, ServerSupervisor,
    SupervisorCommand,
};
use dart_protocol::event::DaemonEvent;
use dart_protocol::instance::InstanceStateDto;
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tokio::sync::{broadcast, mpsc};

/// Spawns a remote supervisor worker connected to `dartd` via the client.
pub async fn spawn_remote_supervisor(
    client: &DartClient,
) -> Result<(ServerSupervisor, broadcast::Receiver<ServerEvent>), Box<dyn std::error::Error>> {
    let (command_tx, mut command_rx) = mpsc::channel::<SupervisorCommand>(64);
    let (event_tx, event_rx) = broadcast::channel::<ServerEvent>(256);
    let states = Arc::new(RwLock::new(HashMap::new()));
    let logs = Arc::new(RwLock::new(HashMap::new()));

    // 1. Sync existing instance states & recent logs
    if let Ok(instances) = client.list_instances().await {
        let mut states_guard = states.write().unwrap();
        let mut logs_guard = logs.write().unwrap();
        for inst in instances {
            if let Ok(id) = InstanceId::from_str(&inst.id) {
                let state = match inst.state {
                    InstanceStateDto::Stopped => InstanceState::Stopped,
                    InstanceStateDto::Starting => InstanceState::Starting,
                    InstanceStateDto::Running { pid } => InstanceState::Running { pid },
                    InstanceStateDto::Stopping => InstanceState::Stopping,
                    InstanceStateDto::Failed { message } => InstanceState::Failed { message },
                };
                states_guard.insert(id.clone(), state);

                if let Ok(lines) = client.get_logs(&inst.id, Some(100)).await {
                    let ring: &mut VecDeque<ConsoleLineRecord> = logs_guard.entry(id).or_default();
                    for line_dto in lines {
                        let stream = match line_dto.stream {
                            dart_protocol::console::OutputStreamDto::Stdout => OutputStream::Stdout,
                            dart_protocol::console::OutputStreamDto::Stderr => OutputStream::Stderr,
                        };
                        ring.push_back(ConsoleLineRecord {
                            stream,
                            line: line_dto.line,
                            timestamp_millis: line_dto.timestamp_millis,
                        });
                    }
                }
            }
        }
    }

    // 2. Subscribe to real-time events from daemon
    let mut daemon_events = client.subscribe_events().await?;

    let client_clone = client.clone();
    let states_clone = states.clone();
    let logs_clone = logs.clone();
    let event_tx_clone = event_tx.clone();

    // 3. Spawn background worker forwarding commands and events
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        SupervisorCommand::Start(instance) => {
                            let _ = client_clone.start_instance(instance.id().as_str()).await;
                        }
                        SupervisorCommand::Stop(id) => {
                            let _ = client_clone.stop_instance(id.as_str()).await;
                        }
                        SupervisorCommand::StopAll => {
                            let active_ids: Vec<InstanceId> = {
                                let guard = states_clone.read().unwrap();
                                guard.iter().filter_map(|(k, v)| {
                                    if matches!(v, InstanceState::Running { .. } | InstanceState::Starting) {
                                        Some(k.clone())
                                    } else {
                                        None
                                    }
                                }).collect()
                            };
                            for id in active_ids {
                                let _ = client_clone.stop_instance(id.as_str()).await;
                            }
                        }
                        SupervisorCommand::KillAll => {
                            let active_ids: Vec<InstanceId> = {
                                let guard = states_clone.read().unwrap();
                                guard.iter().filter_map(|(k, v)| {
                                    if matches!(v, InstanceState::Running { .. } | InstanceState::Starting) {
                                        Some(k.clone())
                                    } else {
                                        None
                                    }
                                }).collect()
                            };
                            for id in active_ids {
                                let _ = client_clone.kill_instance(id.as_str()).await;
                            }
                        }
                        SupervisorCommand::SendConsole { id, command } => {
                            let _ = client_clone.send_command(id.as_str(), &command).await;
                        }
                    }
                }
                Ok(event) = daemon_events.recv() => {
                    match event {
                        DaemonEvent::StateChanged { id, state } => {
                            if let Ok(inst_id) = InstanceId::from_str(&id) {
                                let inst_state = match state {
                                    InstanceStateDto::Stopped => InstanceState::Stopped,
                                    InstanceStateDto::Starting => InstanceState::Starting,
                                    InstanceStateDto::Running { pid } => InstanceState::Running { pid },
                                    InstanceStateDto::Stopping => InstanceState::Stopping,
                                    InstanceStateDto::Failed { message } => InstanceState::Failed { message },
                                };
                                states_clone.write().unwrap().insert(inst_id.clone(), inst_state.clone());
                                let _ = event_tx_clone.send(ServerEvent::StateChanged {
                                    id: inst_id,
                                    state: inst_state,
                                });
                            }
                        }
                        DaemonEvent::ConsoleLine { id, stream, line, timestamp_millis } => {
                            if let Ok(inst_id) = InstanceId::from_str(&id) {
                                let out_stream = match stream {
                                    dart_protocol::console::OutputStreamDto::Stdout => OutputStream::Stdout,
                                    dart_protocol::console::OutputStreamDto::Stderr => OutputStream::Stderr,
                                };
                                {
                                    let mut logs_guard = logs_clone.write().unwrap();
                                    let ring: &mut VecDeque<ConsoleLineRecord> =
                                        logs_guard.entry(inst_id.clone()).or_default();
                                    if ring.len() >= 500 {
                                        ring.pop_front();
                                    }
                                    ring.push_back(ConsoleLineRecord {
                                        stream: out_stream,
                                        line: line.clone(),
                                        timestamp_millis,
                                    });
                                }
                                let _ = event_tx_clone.send(ServerEvent::ConsoleLine {
                                    id: inst_id,
                                    stream: out_stream,
                                    line,
                                });
                            }
                        }
                        DaemonEvent::OperationFailed { id, message } => {
                            if let Ok(inst_id) = InstanceId::from_str(&id) {
                                let _ = event_tx_clone.send(ServerEvent::OperationFailed {
                                    id: inst_id,
                                    message,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                else => break,
            }
        }
    });

    let supervisor = ServerSupervisor::from_channels(command_tx, event_tx, states, logs);
    Ok((supervisor, event_rx))
}
