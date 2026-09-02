//! Command-line parsing and command execution.
//!
//! The parser converts argv into typed commands before any filesystem or
//! network work starts. Keeping it here makes CLI rules testable without a
//! terminal or a Minecraft installation.

use crate::daemon::{
    CreateInstance, Daemon, DartPaths, EulaAcceptance, FabricRuntime, FabricVersion, InstanceId,
    InstanceName, RuntimeRequest,
};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

const HELP: &str = "Dart manages local Fabric server instances.\n\n\
Usage:\n  \
dart [--home <directory>]                           Open the TUI\n  \
dart [--home <directory>] list                      List instances\n  \
dart [--home <directory>] create <id> <name...>\n  \
     [--minecraft <version> | --runtime <mc/loader/installer>] [--accept-eula]\n  \
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
    List,
    Create(CreateCommand),
    Runtimes(RuntimeCommand),
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
    let (daemon, events) = Daemon::new(paths)?;

    match cli.command {
        Command::Tui => {
            crate::tui::run(daemon, events).await?;
        }
        Command::List => print_instances(&daemon)?,
        Command::Create(command) => create_instance(&daemon, command).await?,
        Command::Runtimes(command) => manage_runtimes(&daemon, command).await?,
        Command::Help => unreachable!("help returns before services are built"),
    }
    Ok(())
}

fn print_instances(daemon: &Daemon) -> Result<(), Box<dyn Error>> {
    let instances = daemon.list_instances()?;
    if instances.is_empty() {
        println!(
            "No instances in {}",
            daemon.instance_store().instances_dir().display()
        );
        return Ok(());
    }

    for instance in instances {
        println!(
            "{}\t{}\tMinecraft {}\t{}",
            instance.id(),
            instance.config().name,
            instance.config().fabric.minecraft,
            instance.root().display()
        );
    }
    Ok(())
}

async fn create_instance(daemon: &Daemon, command: CreateCommand) -> Result<(), Box<dyn Error>> {
    let runtime = match command.runtime {
        CreateRuntime::Latest => RuntimeRequest::Latest,
        CreateRuntime::Minecraft(version) => RuntimeRequest::Minecraft(version),
        CreateRuntime::Exact(runtime) => RuntimeRequest::Exact(runtime),
    };
    let instance = daemon
        .create_instance(CreateInstance::new(
            command.id,
            command.name,
            runtime,
            command.eula,
        ))
        .await?;
    println!(
        "Instance '{}' is ready at {} with {}",
        instance.id(),
        instance.root().display(),
        instance.config().fabric.label()
    );
    Ok(())
}

async fn manage_runtimes(daemon: &Daemon, command: RuntimeCommand) -> Result<(), Box<dyn Error>> {
    match command {
        RuntimeCommand::List => {
            let runtimes = daemon.list_runtimes()?;
            if runtimes.is_empty() {
                println!(
                    "No cached Fabric runtimes in {}",
                    daemon.paths().fabric_runtimes_dir().display()
                );
            } else {
                for runtime in runtimes {
                    println!("{runtime}");
                }
            }
        }
        RuntimeCommand::Download(minecraft) => {
            let runtime = daemon
                .resolve_runtime(minecraft.as_ref().map(FabricVersion::as_str))
                .await?;
            daemon.cache_runtime(&runtime).await?;
            println!("Cached {}", runtime.label());
        }
    }
    Ok(())
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
        Some("list") if arguments.len() == 1 => Command::List,
        Some("create") => Command::Create(parse_create(&arguments[1..])?),
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
    match arguments {
        [] => Ok(RuntimeCommand::List),
        [command] if command == "list" => Ok(RuntimeCommand::List),
        [command] if command == "download" => Ok(RuntimeCommand::Download(None)),
        [command, minecraft] if command == "download" => Ok(RuntimeCommand::Download(Some(
            FabricVersion::parse(minecraft).map_err(CliError::InvalidVersion)?,
        ))),
        _ => Err(CliError::Usage(
            "dart runtimes [list | download [minecraft-version]]",
        )),
    }
}

fn parse_runtime(value: &str) -> Result<FabricRuntime, CliError> {
    let mut parts = value.split('/');
    let (Some(minecraft), Some(loader), Some(installer), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CliError::InvalidRuntime(value.to_owned()));
    };
    FabricRuntime::new(minecraft, loader, installer).map_err(CliError::InvalidVersion)
}

fn resolve_home(
    explicit: Option<PathBuf>,
    dart_home: Option<OsString>,
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(path) = non_empty_path(dart_home) {
        return Ok(path);
    }
    if let Some(path) = non_empty_path(xdg_data_home) {
        return Ok(path.join("dart"));
    }
    if let Some(path) = non_empty_path(home) {
        return Ok(path.join(".local/share/dart"));
    }
    Err(CliError::NoHomeDirectory)
}

fn non_empty_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

#[derive(Debug)]
enum CliError {
    NonUnicodeArgument,
    MissingValue(&'static str),
    Usage(&'static str),
    UnknownCommand(String),
    UnknownCreateOption(String),
    EmptyInstanceName,
    ConflictingRuntimeOptions,
    InvalidRuntime(String),
    InvalidInstanceId(crate::instance::InstanceIdError),
    InvalidInstanceName(crate::instance::InstanceValidationError),
    InvalidVersion(crate::runtime::RuntimeError),
    NoHomeDirectory,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUnicodeArgument => formatter.write_str("arguments must be valid Unicode"),
            Self::MissingValue(option) => write!(formatter, "{option} requires a value"),
            Self::Usage(usage) => write!(formatter, "usage: {usage}"),
            Self::UnknownCommand(command) => {
                write!(
                    formatter,
                    "unknown or invalid command '{command}'. Run 'dart help'."
                )
            }
            Self::UnknownCreateOption(option) => {
                write!(formatter, "unknown create option '{option}'")
            }
            Self::EmptyInstanceName => formatter.write_str("instance name cannot be empty"),
            Self::ConflictingRuntimeOptions => {
                formatter.write_str("--minecraft and --runtime cannot be used together")
            }
            Self::InvalidRuntime(runtime) => {
                write!(
                    formatter,
                    "runtime must be <minecraft>/<loader>/<installer>, got '{runtime}'"
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
        assert_eq!(command.eula, crate::instance::EulaAcceptance::Accepted);
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
