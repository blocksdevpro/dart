//! Lifecycle, supervision, and console I/O for managed server processes.

use crate::instance::{Instance, InstanceId, InstanceState};
use crate::runtime::FABRIC_LAUNCHER_FILE;
use std::collections::HashMap;
use std::io;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc};

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 1024;

/// Target stream for console text emitted by a server process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
}

/// Events emitted by server processes and the supervisor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerEvent {
    /// An instance process changed lifecycle state.
    StateChanged {
        /// The instance whose state changed.
        id: InstanceId,
        /// The new lifecycle state.
        state: InstanceState,
    },
    /// A line of console output was emitted by a server process.
    ConsoleLine {
        /// The instance that emitted the output.
        id: InstanceId,
        /// The stream where the line was emitted.
        stream: OutputStream,
        /// The raw text line.
        line: String,
    },
    /// An operational error occurred while supervising an instance.
    OperationFailed {
        /// The instance associated with the failure.
        id: InstanceId,
        /// Explanation of the failure.
        message: String,
    },
}

enum SupervisorCommand {
    Start(Instance),
    Stop(InstanceId),
    StopAll,
    KillAll,
    SendConsole { id: InstanceId, command: String },
}

/// Handle to the background process supervisor that controls Minecraft server processes.
#[derive(Clone)]
pub struct ServerSupervisor {
    commands: mpsc::Sender<SupervisorCommand>,
    events: broadcast::Sender<ServerEvent>,
    states: Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
}

impl ServerSupervisor {
    /// Spawns the supervisor background loop and returns a handle and primary event receiver.
    pub fn spawn() -> (Self, broadcast::Receiver<ServerEvent>) {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (event_tx, event_rx) = broadcast::channel(EVENT_CAPACITY);
        let states = Arc::new(RwLock::new(HashMap::new()));
        tokio::spawn(run_supervisor(
            command_rx,
            event_tx.clone(),
            Arc::clone(&states),
        ));
        (
            Self {
                commands: command_tx,
                events: event_tx,
                states,
            },
            event_rx,
        )
    }

    /// Subscribes to the broadcast stream of server events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
    }

    /// Returns the current lifecycle state of an instance in O(1).
    pub fn state(&self, id: &InstanceId) -> InstanceState {
        self.states
            .read()
            .ok()
            .and_then(|states| states.get(id).cloned())
            .unwrap_or(InstanceState::Stopped)
    }

    /// Starts a managed server instance.
    pub async fn start(&self, instance: Instance) -> Result<(), SupervisorUnavailable> {
        self.send(SupervisorCommand::Start(instance)).await
    }

    /// Gracefully stops a running instance.
    pub async fn stop(&self, id: InstanceId) -> Result<(), SupervisorUnavailable> {
        self.send(SupervisorCommand::Stop(id)).await
    }

    /// Gracefully stops all currently running instances.
    pub async fn stop_all(&self) -> Result<(), SupervisorUnavailable> {
        self.send(SupervisorCommand::StopAll).await
    }

    /// Forcefully terminates all currently running instances.
    pub async fn kill_all(&self) -> Result<(), SupervisorUnavailable> {
        self.send(SupervisorCommand::KillAll).await
    }

    /// Sends a console command string into the standard input of a running instance.
    pub async fn send_console(
        &self,
        id: InstanceId,
        command: String,
    ) -> Result<(), SupervisorUnavailable> {
        self.send(SupervisorCommand::SendConsole { id, command })
            .await
    }

    async fn send(&self, command: SupervisorCommand) -> Result<(), SupervisorUnavailable> {
        self.commands
            .send(command)
            .await
            .map_err(|_| SupervisorUnavailable)
    }
}

/// Error indicating that the background supervisor loop has terminated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupervisorUnavailable;

impl std::fmt::Display for SupervisorUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("server supervisor is unavailable")
    }
}

impl std::error::Error for SupervisorUnavailable {}

struct RunningServer {
    child: Child,
    stdin: ChildStdin,
    state: InstanceState,
}

async fn run_supervisor(
    mut commands: mpsc::Receiver<SupervisorCommand>,
    events: broadcast::Sender<ServerEvent>,
    states: Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
) {
    let mut running = HashMap::<InstanceId, RunningServer>::new();
    let mut exit_check = tokio::time::interval(Duration::from_millis(100));
    exit_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(SupervisorCommand::Start(instance)) => {
                        start_instance(instance, &mut running, &events, &states).await;
                    }
                    Some(SupervisorCommand::Stop(id)) => {
                        stop_instance(&id, &mut running, &events, &states).await;
                    }
                    Some(SupervisorCommand::StopAll) => {
                        let ids: Vec<_> = running.keys().cloned().collect();
                        for id in ids {
                            stop_instance(&id, &mut running, &events, &states).await;
                        }
                    }
                    Some(SupervisorCommand::KillAll) => {
                        kill_all(&mut running, &events, &states);
                    }
                    Some(SupervisorCommand::SendConsole { id, command }) => {
                        send_console(&id, command, &mut running, &events).await;
                    }
                    None => {
                        let ids: Vec<_> = running.keys().cloned().collect();
                        for id in ids {
                            stop_instance(&id, &mut running, &events, &states).await;
                        }
                        if running.is_empty() {
                            break;
                        }
                    }
                }
            }
            _ = exit_check.tick() => {
                reap_exited(&mut running, &events, &states);
                if commands.is_closed() && running.is_empty() {
                    break;
                }
            }
        }
    }
}

async fn start_instance(
    instance: Instance,
    running: &mut HashMap<InstanceId, RunningServer>,
    events: &broadcast::Sender<ServerEvent>,
    states: &Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
) {
    let id = instance.id().clone();
    if let Some(server) = running.get(&id) {
        send_state(events, states, id, server.state.clone());
        return;
    }

    send_state(events, states, id.clone(), InstanceState::Starting);
    let jar = instance.root().join(FABRIC_LAUNCHER_FILE);
    if !jar.is_file() {
        send_state(
            events,
            states,
            id,
            InstanceState::Failed {
                message: format!("Fabric launcher JAR is missing: {}", jar.display()),
            },
        );
        return;
    }

    let launch = &instance.config().launch;
    let mut command = Command::new(&launch.java);
    command
        .current_dir(instance.root())
        .arg(format!("-Xms{}M", launch.min_memory_mib))
        .arg(format!("-Xmx{}M", launch.max_memory_mib))
        .arg("-jar")
        .arg(FABRIC_LAUNCHER_FILE)
        .arg("nogui")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = start_error_message(&instance, &error);
            send_state(events, states, id, InstanceState::Failed { message });
            return;
        }
    };

    let Some(stdin) = child.stdin.take() else {
        send_state(
            events,
            states,
            id,
            InstanceState::Failed {
                message: "Fabric standard input was not captured".to_owned(),
            },
        );
        let _ = child.kill().await;
        return;
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let state = match child.id() {
        Some(pid) => InstanceState::Running { pid },
        None => InstanceState::Failed {
            message: "Fabric started without a process ID".to_owned(),
        },
    };
    running.insert(
        id.clone(),
        RunningServer {
            child,
            stdin,
            state: state.clone(),
        },
    );
    send_state(events, states, id.clone(), state);
    if let Some(stdout) = stdout {
        tokio::spawn(read_output(
            id.clone(),
            OutputStream::Stdout,
            stdout,
            events.clone(),
        ));
    }
    if let Some(stderr) = stderr {
        tokio::spawn(read_output(
            id,
            OutputStream::Stderr,
            stderr,
            events.clone(),
        ));
    }
}

fn start_error_message(instance: &Instance, error: &io::Error) -> String {
    let java = instance.config().launch.java.display();
    match error.kind() {
        io::ErrorKind::NotFound => {
            let requirement = if instance
                .config()
                .fabric
                .minecraft
                .as_str()
                .starts_with("26.")
            {
                format!(
                    " Minecraft {} requires Java 25.",
                    instance.config().fabric.minecraft
                )
            } else {
                String::new()
            };
            format!(
                "Java executable '{java}' was not found.{requirement} Install Java or set 'java' in {} to its executable path.",
                instance.root().join("dart.toml").display()
            )
        }
        io::ErrorKind::PermissionDenied => format!(
            "Java executable '{java}' is not executable. Check its permissions or update 'java' in {}.",
            instance.root().join("dart.toml").display()
        ),
        _ => format!("cannot start Java executable '{java}': {error}"),
    }
}

async fn stop_instance(
    id: &InstanceId,
    running: &mut HashMap<InstanceId, RunningServer>,
    events: &broadcast::Sender<ServerEvent>,
    states: &Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
) {
    let Some(server) = running.get_mut(id) else {
        send_state(events, states, id.clone(), InstanceState::Stopped);
        return;
    };
    if matches!(server.state, InstanceState::Stopping) {
        return;
    }

    match write_command(&mut server.stdin, "stop").await {
        Ok(()) => {
            server.state = InstanceState::Stopping;
            send_state(events, states, id.clone(), InstanceState::Stopping);
        }
        Err(error) => {
            send_failure(
                events,
                id.clone(),
                format!("cannot send stop command; terminating the process: {error}"),
            );
            let state = InstanceState::Stopping;
            server.state = state.clone();
            send_state(events, states, id.clone(), state);
            if let Err(kill_error) = server.child.start_kill() {
                send_failure(
                    events,
                    id.clone(),
                    format!("cannot terminate Fabric: {kill_error}"),
                );
            }
        }
    }
}

fn kill_all(
    running: &mut HashMap<InstanceId, RunningServer>,
    events: &broadcast::Sender<ServerEvent>,
    states: &Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
) {
    for (id, server) in running.iter_mut() {
        match server.child.start_kill() {
            Ok(()) => {
                server.state = InstanceState::Stopping;
                send_state(events, states, id.clone(), InstanceState::Stopping);
            }
            Err(error) => {
                send_failure(
                    events,
                    id.clone(),
                    format!("cannot terminate Fabric: {error}"),
                );
            }
        }
    }
}

async fn send_console(
    id: &InstanceId,
    command: String,
    running: &mut HashMap<InstanceId, RunningServer>,
    events: &broadcast::Sender<ServerEvent>,
) {
    if command.trim().is_empty() {
        return;
    }
    let Some(server) = running.get_mut(id) else {
        send_failure(events, id.clone(), "instance is not running".to_owned());
        return;
    };
    if !matches!(server.state, InstanceState::Running { .. }) {
        send_failure(
            events,
            id.clone(),
            "instance is not ready for console input".to_owned(),
        );
        return;
    }

    if let Err(error) = write_command(&mut server.stdin, command.trim()).await {
        send_failure(
            events,
            id.clone(),
            format!("cannot send console command: {error}"),
        );
    }
}

async fn write_command(stdin: &mut ChildStdin, command: &str) -> io::Result<()> {
    stdin.write_all(command.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

fn reap_exited(
    running: &mut HashMap<InstanceId, RunningServer>,
    events: &broadcast::Sender<ServerEvent>,
    states: &Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
) {
    let mut exited = Vec::new();
    for (id, server) in running.iter_mut() {
        match server.child.try_wait() {
            Ok(Some(status)) => exited.push((id.clone(), Ok(status))),
            Ok(None) => {}
            Err(error) => exited.push((id.clone(), Err(error))),
        }
    }

    for (id, result) in exited {
        running.remove(&id);
        let state = match result {
            Ok(status) if status.success() => InstanceState::Stopped,
            Ok(status) => InstanceState::Failed {
                message: format!("Fabric exited with {status}"),
            },
            Err(error) => InstanceState::Failed {
                message: format!("cannot read Fabric process status: {error}"),
            },
        };
        send_state(events, states, id, state);
    }
}

async fn read_output<R>(
    id: InstanceId,
    stream: OutputStream,
    reader: R,
    events: broadcast::Sender<ServerEvent>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = events.send(ServerEvent::ConsoleLine {
                    id: id.clone(),
                    stream,
                    line,
                });
            }
            Ok(None) => break,
            Err(error) => {
                send_failure(
                    &events,
                    id.clone(),
                    format!("cannot read Fabric output: {error}"),
                );
                break;
            }
        }
    }
}

fn send_state(
    events: &broadcast::Sender<ServerEvent>,
    states: &Arc<RwLock<HashMap<InstanceId, InstanceState>>>,
    id: InstanceId,
    state: InstanceState,
) {
    if let Ok(mut lock) = states.write() {
        if state == InstanceState::Stopped {
            lock.remove(&id);
        } else {
            lock.insert(id.clone(), state.clone());
        }
    }
    let _ = events.send(ServerEvent::StateChanged { id, state });
}

fn send_failure(events: &broadcast::Sender<ServerEvent>, id: InstanceId, message: String) {
    let _ = events.send(ServerEvent::OperationFailed { id, message });
}

#[cfg(all(test, unix))]
mod tests {
    use super::{OutputStream, ServerEvent, ServerSupervisor};
    use crate::instance::{
        FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName, InstanceState,
    };
    use crate::runtime::FabricRuntime;
    use crate::testing::TestDirectory;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::str::FromStr;
    use std::time::Duration;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn streams_console_commands_and_stops_gracefully() {
        let directory = TestDirectory::new("supervisor");
        let fake_java = directory.path().join("fake-java.sh");
        fs::write(
            &fake_java,
            "#!/bin/sh\necho ready\nwhile IFS= read -r line; do\n  echo command:$line\n  [ \"$line\" = stop ] && exit 0\ndone\n",
        )
        .unwrap();
        fs::set_permissions(&fake_java, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(directory.path().join("fabric-server-launch.jar"), "fixture").unwrap();

        let id = InstanceId::from_str("test-server").unwrap();
        let launch = FabricLaunch {
            java: fake_java,
            min_memory_mib: 64,
            max_memory_mib: 128,
        };
        let instance = Instance::new(
            id.clone(),
            directory.path().to_owned(),
            InstanceConfig::new(
                InstanceName::parse("Test server").unwrap(),
                launch,
                FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
            ),
        );
        let (supervisor, mut events) = ServerSupervisor::spawn();

        supervisor.start(instance).await.unwrap();
        wait_for(&mut events, |event| {
            matches!(
                event,
                ServerEvent::StateChanged {
                    state: InstanceState::Running { .. },
                    ..
                }
            )
        })
        .await;
        wait_for(&mut events, |event| {
            matches!(
                event,
                ServerEvent::ConsoleLine {
                    stream: OutputStream::Stdout,
                    line,
                    ..
                } if line == "ready"
            )
        })
        .await;

        supervisor
            .send_console(id.clone(), "say hello".to_owned())
            .await
            .unwrap();
        wait_for(&mut events, |event| {
            matches!(
                event,
                ServerEvent::ConsoleLine { line, .. } if line == "command:say hello"
            )
        })
        .await;

        supervisor.stop(id).await.unwrap();
        wait_for(&mut events, |event| {
            matches!(
                event,
                ServerEvent::StateChanged {
                    state: InstanceState::Stopped,
                    ..
                }
            )
        })
        .await;
    }

    #[tokio::test]
    async fn explains_when_the_java_executable_is_missing() {
        let directory = TestDirectory::new("supervisor");
        fs::write(directory.path().join("fabric-server-launch.jar"), "fixture").unwrap();
        let id = InstanceId::from_str("missing-java").unwrap();
        let missing_java = directory.path().join("java-does-not-exist");
        let instance = Instance::new(
            id,
            directory.path().to_owned(),
            InstanceConfig::new(
                InstanceName::parse("Missing Java").unwrap(),
                FabricLaunch {
                    java: missing_java,
                    min_memory_mib: 64,
                    max_memory_mib: 128,
                },
                FabricRuntime::new("26.2", "0.19.3", "1.1.2").unwrap(),
            ),
        );
        let (supervisor, mut events) = ServerSupervisor::spawn();

        supervisor.start(instance).await.unwrap();
        let event = wait_for(&mut events, |event| {
            matches!(
                event,
                ServerEvent::StateChanged {
                    state: InstanceState::Failed { .. },
                    ..
                }
            )
        })
        .await;

        let ServerEvent::StateChanged {
            state: InstanceState::Failed { message },
            ..
        } = event
        else {
            panic!("expected a failed state");
        };
        assert!(message.contains("Java executable"));
        assert!(message.contains("was not found"));
        assert!(message.contains("Minecraft 26.2 requires Java 25"));
        assert!(message.contains("dart.toml"));
    }

    async fn wait_for(
        events: &mut broadcast::Receiver<ServerEvent>,
        predicate: impl Fn(&ServerEvent) -> bool,
    ) -> ServerEvent {
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Ok(event) = events.recv().await {
                if predicate(&event) {
                    return event;
                }
            }
            panic!("event channel closed");
        })
        .await
        .expect("timed out waiting for supervisor event")
    }
}
