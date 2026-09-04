//! Command-line parsing and command execution.
//!
//! The parser converts argv into typed commands before any filesystem or
//! network work starts. Keeping it here makes CLI rules testable without a
//! terminal or a Minecraft installation.

mod daemon;
mod remote_supervisor;

use daemon::ensure_daemon;
use dart_client::DartClient;
use dart_daemon::{
    Daemon, DartPaths, EulaAcceptance, FabricRuntime, FabricVersion, InstanceId,
    InstanceName,
};
use dart_protocol::instance::{CreateInstanceRequest, InstanceDto, InstanceStateDto};
use dart_protocol::runtime::DownloadRuntimeRequest;
use remote_supervisor::spawn_remote_supervisor;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

const HELP: &str = "Dart manages local Fabric server instances.\n\n\
Usage:\n  \
dart [--home <directory>]                           Open the TUI\n  \
dart [--home <directory>] status                    Show daemon status and health\n  \
dart [--home <directory>] daemon [status|start|stop] Control the background daemon\n  \
dart [--home <directory>] list                      List instances\n  \
dart [--home <directory>] create <id> <name...>\n  \
     [--minecraft <version> | --runtime <mc/loader/installer>] [--accept-eula]\n  \
dart [--home <directory>] start <id>                Start an instance\n  \
dart [--home <directory>] stop <id>                 Stop an instance\n  \
dart [--home <directory>] restart <id>              Restart an instance\n  \
dart [--home <directory>] logs <id> [tail]          View recent console logs\n  \
dart [--home <directory>] runtimes list             List cached launchers\n  \
dart [--home <directory>] runtimes download [mc]    Cache a launcher\n  \
dart help                                           Show this help\n\n\
Create downloads the newest stable Fabric runtime by default.\n\
DART_HOME overrides the default data directory.";

#[derive(Debug, Eq, PartialEq)]
struct Cli {
    home: Option<PathBuf>,
    command: Command,
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Tui,
    Help,
    Status,
    Daemon(DaemonCommand),
    List,
    Create(CreateCommand),
    Runtimes(RuntimeCommand),
    Start(InstanceId),
    Stop(InstanceId),
    Restart(InstanceId),
    Logs { id: InstanceId, tail: Option<usize> },
}

#[derive(Debug, Eq, PartialEq)]
enum DaemonCommand {
    Status,
    Start,
    Stop,
}

#[derive(Debug, Eq, PartialEq)]
struct CreateCommand {
    id: InstanceId,
    name: InstanceName,
    runtime: CreateRuntime,
    eula: EulaAcceptance,
}

#[derive(Debug, Eq, PartialEq)]
enum CreateRuntime {
    Latest,
    Minecraft(FabricVersion),
    Exact(FabricRuntime),
}

#[derive(Debug, Eq, PartialEq)]
enum RuntimeCommand {
    List,
    Download(Option<FabricVersion>),
}

pub async fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let cli = parse(arguments)?;
    if cli.command == Command::Help {
        println!("{HELP}");
        return Ok(());
    }

    let paths = DartPaths::new(resolve_home(
        cli.home,
        env::var_os("DART_HOME"),
        env::var_os("XDG_DATA_HOME"),
        env::var_os("HOME"),
    )?);

    if cli.command == Command::Status || cli.command == Command::Daemon(DaemonCommand::Status) {
        return print_daemon_status(&paths).await;
    }

    if cli.command == Command::Daemon(DaemonCommand::Stop) {
        return stop_daemon(&paths).await;
    }

    let client = ensure_daemon(&paths).await?;

    if cli.command == Command::Daemon(DaemonCommand::Start) {
        println!("Dart daemon is running.");
        return print_daemon_status(&paths).await;
    }

    match cli.command {
        Command::Tui => {
            let (daemon, _local_events) = Daemon::new(paths.clone())?;
            let (supervisor, events) = spawn_remote_supervisor(&client).await?;
            let instance_service = daemon.instance_service().clone();
            let content = daemon.content_manager().clone();
            let daemon = Daemon::from_components(paths, instance_service, supervisor, content);
            crate::tui::run(daemon, events).await?;
        }
        Command::List => print_instances(&client, &paths).await?,
        Command::Create(command) => create_instance(&client, command).await?,
        Command::Runtimes(command) => manage_runtimes(&client, &paths, command).await?,
        Command::Start(id) => {
            client.start_instance(id.as_str()).await?;
            println!("Started instance '{id}'.");
        }
        Command::Stop(id) => {
            client.stop_instance(id.as_str()).await?;
            println!("Stopped instance '{id}'.");
        }
        Command::Restart(id) => {
            client.restart_instance(id.as_str()).await?;
            println!("Restarted instance '{id}'.");
        }
        Command::Logs { id, tail } => {
            let lines = client.get_logs(id.as_str(), tail).await?;
            if lines.is_empty() {
                println!("No recent logs for instance '{id}'.");
            } else {
                for line in lines {
                    let stream_prefix = match line.stream {
                        dart_protocol::console::OutputStreamDto::Stdout => "",
                        dart_protocol::console::OutputStreamDto::Stderr => "[stderr] ",
                    };
                    println!("{stream_prefix}{}", line.line);
                }
            }
        }
        Command::Help => unreachable!("help returns before services are built"),
        Command::Status | Command::Daemon(_) => unreachable!("handled before daemon connection"),
    }
    Ok(())
}

async fn print_instances(client: &DartClient, paths: &DartPaths) -> Result<(), Box<dyn Error>> {
    let instances: Vec<InstanceDto> = client.list_instances().await?;
    if instances.is_empty() {
        println!(
            "No instances in {}",
            paths.instances_dir().display()
        );
        return Ok(());
    }

    for instance in instances {
        let state_suffix = match &instance.state {
            InstanceStateDto::Running { pid } => format!("  [RUNNING: PID {pid}]"),
            InstanceStateDto::Starting => "  [STARTING]".to_string(),
            InstanceStateDto::Stopping => "  [STOPPING]".to_string(),
            InstanceStateDto::Failed { message } => format!("  [FAILED: {message}]"),
            InstanceStateDto::Stopped => String::new(),
        };
        println!(
            "{}\t{}\tMinecraft {}\t{}{}",
            instance.id,
            instance.config.name,
            instance.config.fabric.minecraft,
            instance.root,
            state_suffix
        );
    }
    Ok(())
}

async fn create_instance(client: &DartClient, command: CreateCommand) -> Result<(), Box<dyn Error>> {
    let (mc, loader, installer) = match command.runtime {
        CreateRuntime::Latest => (None, None, None),
        CreateRuntime::Minecraft(version) => (Some(version.to_string()), None, None),
        CreateRuntime::Exact(runtime) => (
            Some(runtime.minecraft.to_string()),
            Some(runtime.loader.to_string()),
            Some(runtime.installer.to_string()),
        ),
    };

    let req = CreateInstanceRequest {
        id: command.id.to_string(),
        name: command.name.to_string(),
        minecraft: mc,
        loader,
        installer,
        accept_eula: command.eula.is_accepted(),
        min_memory_mib: None,
        max_memory_mib: None,
        java: None,
    };

    let instance = client.create_instance(&req).await?;
    let label = format!(
        "Minecraft {}, loader {}, installer {}",
        instance.config.fabric.minecraft,
        instance.config.fabric.loader,
        instance.config.fabric.installer
    );
    println!(
        "Instance '{}' is ready at {} with {}",
        instance.id,
        instance.root,
        label
    );
    Ok(())
}

async fn manage_runtimes(
    client: &DartClient,
    paths: &DartPaths,
    command: RuntimeCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        RuntimeCommand::List => {
            let runtimes = client.list_runtimes().await?;
            if runtimes.is_empty() {
                println!(
                    "No cached Fabric runtimes in {}",
                    paths.fabric_runtimes_dir().display()
                );
            } else {
                for runtime in runtimes {
                    println!(
                        "Minecraft {}, loader {}, installer {}",
                        runtime.minecraft, runtime.loader, runtime.installer
                    );
                }
            }
        }
        RuntimeCommand::Download(minecraft) => {
            let req = DownloadRuntimeRequest {
                minecraft: minecraft.map(|v| v.to_string()),
                loader: None,
                installer: None,
            };
            let runtime = client.download_runtime(&req).await?;
            println!(
                "Cached Minecraft {}, loader {}, installer {}",
                runtime.minecraft, runtime.loader, runtime.installer
            );
        }
    }
    Ok(())
}

async fn print_daemon_status(paths: &DartPaths) -> Result<(), Box<dyn Error>> {
    let client = DartClient::unix(paths.socket_path());
    match client.health().await {
        Ok(health) => {
            let info = client.system_info().await.ok();
            let instances = client.list_instances().await.unwrap_or_default();
            let active_count = instances
                .iter()
                .filter(|i| {
                    matches!(
                        i.state,
                        InstanceStateDto::Running { .. } | InstanceStateDto::Starting
                    )
                })
                .count();

            let pid_str = info
                .as_ref()
                .map(|i| format!(" (PID: {})", i.pid))
                .unwrap_or_default();
            let uptime_str = format_uptime(health.uptime_seconds);

            println!("Dart Daemon: Running{pid_str}");
            println!("Status:      Healthy (uptime: {uptime_str})");
            println!("Data Home:   {}", paths.home().display());
            println!("Unix Socket: {}", paths.socket_path().display());
            println!(
                "Instances:   {} managed ({} active)",
                instances.len(),
                active_count
            );
            if let Some(info) = info {
                println!("Runtimes:    {} cached", info.runtimes_count);
            }
        }
        Err(_) => {
            println!("Dart Daemon: Not running");
            println!("Data Home:   {}", paths.home().display());
            println!("Unix Socket: {} (inactive)", paths.socket_path().display());
            println!("\nRun 'dart' or 'dart daemon start' to launch the background service.");
        }
    }
    Ok(())
}

async fn stop_daemon(paths: &DartPaths) -> Result<(), Box<dyn Error>> {
    let client = DartClient::unix(paths.socket_path());
    match client.health().await {
        Ok(_) => {
            client.shutdown().await?;
            println!("Dart daemon shutdown signal sent. Stopping running instances...");
        }
        Err(_) => {
            println!("Dart daemon is not running.");
        }
    }
    Ok(())
}

fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Cli, CliError> {
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| CliError::NonUnicodeArgument)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (home, arguments) = parse_home(arguments)?;
    let command = match arguments.first().map(String::as_str) {
        None => Command::Tui,
        Some("tui") if arguments.len() == 1 => Command::Tui,
        Some("help") | Some("--help") | Some("-h") if arguments.len() == 1 => Command::Help,
        Some("status") if arguments.len() == 1 => Command::Status,
        Some("daemon") => Command::Daemon(parse_daemon(&arguments[1..])?),
        Some("list") if arguments.len() == 1 => Command::List,
        Some("create") => Command::Create(parse_create(&arguments[1..])?),
        Some("start") if arguments.len() == 2 => {
            let id = InstanceId::from_str(&arguments[1]).map_err(CliError::InvalidInstanceId)?;
            Command::Start(id)
        }
        Some("stop") if arguments.len() == 2 => {
            let id = InstanceId::from_str(&arguments[1]).map_err(CliError::InvalidInstanceId)?;
            Command::Stop(id)
        }
        Some("restart") if arguments.len() == 2 => {
            let id = InstanceId::from_str(&arguments[1]).map_err(CliError::InvalidInstanceId)?;
            Command::Restart(id)
        }
        Some("logs") if arguments.len() >= 2 => {
            let id = InstanceId::from_str(&arguments[1]).map_err(CliError::InvalidInstanceId)?;
            let tail = arguments.get(2).and_then(|s| s.parse().ok());
            Command::Logs { id, tail }
        }
        Some("runtimes") => Command::Runtimes(parse_runtimes(&arguments[1..])?),
        Some(command) => return Err(CliError::UnknownCommand(command.to_owned())),
    };

    Ok(Cli { home, command })
}

fn parse_home(arguments: Vec<String>) -> Result<(Option<PathBuf>, Vec<String>), CliError> {
    if arguments
        .first()
        .is_some_and(|argument| argument == "--home")
    {
        let Some(path) = arguments.get(1) else {
            return Err(CliError::MissingValue("--home"));
        };
        return Ok((Some(PathBuf::from(path)), arguments[2..].to_vec()));
    }
    Ok((None, arguments))
}

fn parse_create(arguments: &[String]) -> Result<CreateCommand, CliError> {
    if arguments.len() < 2 {
        return Err(CliError::Usage(
            "dart create <id> <name...> [--minecraft <version> | --runtime <mc/loader/installer>] [--accept-eula]",
        ));
    }
    let id = InstanceId::from_str(&arguments[0]).map_err(CliError::InvalidInstanceId)?;
    let option_start = arguments
        .iter()
        .position(|argument| argument.starts_with("--"))
        .unwrap_or(arguments.len());
    if option_start == 1 {
        return Err(CliError::EmptyInstanceName);
    }
    let name = InstanceName::parse(arguments[1..option_start].join(" "))
        .map_err(CliError::InvalidInstanceName)?;

    let mut minecraft = None;
    let mut exact_runtime = None;
    let mut eula = EulaAcceptance::NotAccepted;
    let mut index = option_start;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--minecraft" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or(CliError::MissingValue("--minecraft"))?;
                minecraft = Some(FabricVersion::parse(value).map_err(CliError::InvalidVersion)?);
                index += 2;
            }
            "--runtime" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or(CliError::MissingValue("--runtime"))?;
                exact_runtime = Some(parse_runtime(value)?);
                index += 2;
            }
            "--accept-eula" => {
                eula = EulaAcceptance::Accepted;
                index += 1;
            }
            option => return Err(CliError::UnknownCreateOption(option.to_owned())),
        }
    }
    if minecraft.is_some() && exact_runtime.is_some() {
        return Err(CliError::ConflictingRuntimeOptions);
    }

    let runtime = match (minecraft, exact_runtime) {
        (Some(minecraft), None) => CreateRuntime::Minecraft(minecraft),
        (None, Some(runtime)) => CreateRuntime::Exact(runtime),
        (None, None) => CreateRuntime::Latest,
        (Some(_), Some(_)) => unreachable!("conflict returns above"),
    };
    Ok(CreateCommand {
        id,
        name,
        runtime,
        eula,
    })
}

fn parse_runtimes(arguments: &[String]) -> Result<RuntimeCommand, CliError> {
    match arguments.first().map(String::as_str) {
        Some("list") if arguments.len() == 1 => Ok(RuntimeCommand::List),
        Some("download") if arguments.len() == 1 => Ok(RuntimeCommand::Download(None)),
        Some("download") if arguments.len() == 2 => Ok(RuntimeCommand::Download(Some(
            FabricVersion::parse(&arguments[1]).map_err(CliError::InvalidVersion)?,
        ))),
        _ => Err(CliError::Usage(
            "dart runtimes list | dart runtimes download [minecraft-version]",
        )),
    }
}

fn parse_daemon(arguments: &[String]) -> Result<DaemonCommand, CliError> {
    match arguments.first().map(String::as_str) {
        None | Some("status") if arguments.len() <= 1 => Ok(DaemonCommand::Status),
        Some("start") if arguments.len() == 1 => Ok(DaemonCommand::Start),
        Some("stop") if arguments.len() == 1 => Ok(DaemonCommand::Stop),
        _ => Err(CliError::Usage("dart daemon [status|start|stop]")),
    }
}

fn parse_runtime(value: &str) -> Result<FabricRuntime, CliError> {
    let mut parts = value.split('/');
    let (Some(minecraft), Some(loader), Some(installer), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CliError::InvalidExactRuntime(value.to_owned()));
    };

    FabricRuntime::new(minecraft, loader, installer).map_err(CliError::InvalidRuntime)
}

fn resolve_home(
    explicit: Option<PathBuf>,
    dart_home: Option<OsString>,
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, CliError> {
    if let Some(explicit) = explicit {
        return Ok(explicit);
    }
    if let Some(dart_home) = dart_home {
        return Ok(PathBuf::from(dart_home));
    }
    if let Some(xdg_data_home) = xdg_data_home {
        return Ok(PathBuf::from(xdg_data_home).join("dart"));
    }
    if let Some(home) = home {
        return Ok(PathBuf::from(home).join(".local/share/dart"));
    }
    Err(CliError::NoHomeDirectory)
}

#[derive(Debug)]
enum CliError {
    NonUnicodeArgument,
    MissingValue(&'static str),
    UnknownCommand(String),
    UnknownCreateOption(String),
    EmptyInstanceName,
    ConflictingRuntimeOptions,
    Usage(&'static str),
    InvalidExactRuntime(String),
    InvalidRuntime(crate::daemon::RuntimeError),
    InvalidInstanceId(crate::daemon::InstanceIdError),
    InvalidInstanceName(crate::daemon::InstanceValidationError),
    InvalidVersion(crate::daemon::RuntimeError),
    NoHomeDirectory,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUnicodeArgument => formatter.write_str("arguments must be valid unicode"),
            Self::MissingValue(flag) => write!(formatter, "missing value for {flag}"),
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
            Self::UnknownCreateOption(option) => {
                write!(formatter, "unknown create option: {option}")
            }
            Self::EmptyInstanceName => formatter.write_str("instance name cannot be empty"),
            Self::ConflictingRuntimeOptions => {
                formatter.write_str("--minecraft and --runtime cannot be used together")
            }
            Self::Usage(usage) => write!(formatter, "usage: {usage}"),
            Self::InvalidExactRuntime(value) => write!(
                formatter,
                "invalid exact runtime format '{value}'; expected minecraft/loader/installer"
            ),
            Self::InvalidRuntime(error) => {
                write!(
                    formatter,
                    "invalid runtime: {}",
                    error.to_string().to_lowercase()
                )
            }
            Self::InvalidInstanceId(error) => write!(formatter, "invalid instance ID: {error}"),
            Self::InvalidInstanceName(error) => error.fmt(formatter),
            Self::InvalidVersion(error) => error.fmt(formatter),
            Self::NoHomeDirectory => formatter
                .write_str("cannot determine Dart's data directory; set DART_HOME or use --home"),
        }
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::{Command, CreateRuntime, RuntimeCommand, parse, resolve_home};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_a_cached_runtime_create_command() {
        let cli = parse(arguments(&[
            "--home",
            "data",
            "create",
            "survival",
            "Survival Server",
            "--runtime",
            "1.21.8/0.17.2/1.1.2",
            "--accept-eula",
        ]))
        .unwrap();

        assert_eq!(cli.home, Some(PathBuf::from("data")));
        let Command::Create(command) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(command.id.as_str(), "survival");
        assert_eq!(command.name.to_string(), "Survival Server");
        assert_eq!(command.eula, crate::daemon::EulaAcceptance::Accepted);
        assert!(matches!(command.runtime, CreateRuntime::Exact(_)));
    }

    #[test]
    fn rejects_conflicting_create_runtime_options() {
        let error = parse(arguments(&[
            "create",
            "survival",
            "Survival",
            "--minecraft",
            "1.21.8",
            "--runtime",
            "1.21.8/0.17.2/1.1.2",
        ]))
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "--minecraft and --runtime cannot be used together"
        );
    }

    #[test]
    fn parses_runtime_subcommands() {
        let cli = parse(arguments(&["runtimes", "download", "1.21.8"])).unwrap();
        assert!(matches!(
            cli.command,
            Command::Runtimes(RuntimeCommand::Download(Some(_)))
        ));
    }

    #[test]
    fn parses_start_and_stop_commands() {
        let start_cli = parse(arguments(&["start", "survival"])).unwrap();
        assert!(matches!(start_cli.command, Command::Start(_)));

        let stop_cli = parse(arguments(&["stop", "survival"])).unwrap();
        assert!(matches!(stop_cli.command, Command::Stop(_)));
    }

    #[test]
    fn picks_the_first_available_data_home() {
        assert_eq!(
            resolve_home(
                Some(PathBuf::from("explicit")),
                Some(OsString::from("dart-home")),
                Some(OsString::from("xdg")),
                Some(OsString::from("home")),
            )
            .unwrap(),
            PathBuf::from("explicit")
        );
        assert_eq!(
            resolve_home(
                None,
                None,
                Some(OsString::from("xdg")),
                Some(OsString::from("home")),
            )
            .unwrap(),
            PathBuf::from("xdg/dart")
        );
    }
}
