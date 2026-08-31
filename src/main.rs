use dart::content::ContentManager;
use dart::instance::{EulaAcceptance, FabricLaunch, InstanceConfig, InstanceId, InstanceName};
use dart::mods::{ModManager, ModStore, ModrinthClient};
use dart::packs::PackManager;
use dart::runtime::{FabricClient, FabricRuntime, RuntimeStore};
use dart::store::InstanceStore;
use dart::supervisor::ServerSupervisor;
use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dart: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1).collect::<Vec<_>>();
    let explicit_home = if arguments
        .first()
        .is_some_and(|argument| argument == "--home")
    {
        if arguments.len() < 2 {
            return Err("--home requires a directory".into());
        }
        let path = PathBuf::from(arguments.remove(1));
        arguments.remove(0);
        Some(path)
    } else {
        None
    };
    let dart_home = resolve_dart_home(explicit_home)?;
    let store = InstanceStore::new(dart_home.clone());
    let runtime_store = RuntimeStore::new(dart_home);

    match arguments.first().map(String::as_str) {
        None | Some("tui") => {
            let (supervisor, events) = ServerSupervisor::spawn();
            dart::ui::run(
                store,
                runtime_store,
                FabricClient::new()?,
                ContentManager::new(
                    ModManager::new(ModrinthClient::new()?, ModStore::new()),
                    PackManager::new()?,
                ),
                supervisor,
                events,
            )
            .await
        }
        Some("list") if arguments.len() == 1 => list_instances(&store),
        Some("create") => create_instance(&store, &runtime_store, &arguments[1..]).await,
        Some("runtimes") => manage_runtimes(&runtime_store, &arguments[1..]).await,
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(command) => {
            Err(format!("unknown or invalid command '{command}'. Run 'dart help'.").into())
        }
    }
}

fn resolve_dart_home(explicit: Option<PathBuf>) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(path) = env::var_os("DART_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("dart"));
    }
    if let Some(path) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join(".local/share/dart"));
    }
    Err("cannot determine Dart's data directory; set DART_HOME or use --home".into())
}

fn list_instances(store: &InstanceStore) -> Result<(), Box<dyn Error>> {
    let instances = store.list()?;
    if instances.is_empty() {
        println!("No instances in {}", store.instances_dir().display());
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

async fn create_instance(
    store: &InstanceStore,
    runtime_store: &RuntimeStore,
    arguments: &[String],
) -> Result<(), Box<dyn Error>> {
    if arguments.len() < 2 {
        return Err(
            "usage: dart create <id> <name...> [--minecraft <version> | --runtime <mc/loader/installer>] [--accept-eula]"
                .into(),
        );
    }
    let id = InstanceId::from_str(&arguments[0])?;
    let option_index = arguments
        .iter()
        .position(|argument| argument.starts_with("--"))
        .unwrap_or(arguments.len());
    if option_index == 1 {
        return Err("instance name cannot be empty".into());
    }
    let name = InstanceName::parse(arguments[1..option_index].join(" "))?;
    let client = FabricClient::new()?;

    let mut minecraft = None;
    let mut exact_runtime = None;
    let mut eula = EulaAcceptance::NotAccepted;
    let mut index = option_index;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--minecraft" => {
                let version = arguments
                    .get(index + 1)
                    .ok_or("--minecraft requires a version")?;
                minecraft = Some(version.clone());
                index += 2;
            }
            "--runtime" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or("--runtime requires <minecraft>/<loader>/<installer>")?;
                exact_runtime = Some(parse_runtime(value)?);
                index += 2;
            }
            "--accept-eula" => {
                eula = EulaAcceptance::Accepted;
                index += 1;
            }
            option => return Err(format!("unknown create option '{option}'").into()),
        }
    }
    if minecraft.is_some() && exact_runtime.is_some() {
        return Err("--minecraft and --runtime cannot be used together".into());
    }

    let runtime = if let Some(runtime) = exact_runtime {
        client.download(&runtime, runtime_store).await?;
        runtime
    } else {
        client
            .resolve_and_download(minecraft.as_deref(), runtime_store)
            .await?
    };
    let config = InstanceConfig::new(name, FabricLaunch::default(), runtime.clone());
    let launcher = runtime_store.launcher_path(&runtime);
    let instance = store.create(id, config, &launcher, eula)?;
    println!(
        "Instance '{}' is ready at {} with {}",
        instance.id(),
        instance.root().display(),
        runtime.label()
    );
    Ok(())
}

async fn manage_runtimes(
    runtime_store: &RuntimeStore,
    arguments: &[String],
) -> Result<(), Box<dyn Error>> {
    match arguments.first().map(String::as_str) {
        None | Some("list") if arguments.len() <= 1 => {
            let runtimes = runtime_store.list()?;
            if runtimes.is_empty() {
                println!(
                    "No cached Fabric runtimes in {}",
                    runtime_store.fabric_dir().display()
                );
            } else {
                for runtime in runtimes {
                    println!(
                        "{runtime}\t{}",
                        runtime_store.launcher_path(&runtime).display()
                    );
                }
            }
            Ok(())
        }
        Some("download") if arguments.len() <= 2 => {
            let minecraft = arguments.get(1).map(String::as_str);
            let client = FabricClient::new()?;
            let runtime = client
                .resolve_and_download(minecraft, runtime_store)
                .await?;
            println!(
                "Cached {} at {}",
                runtime.label(),
                runtime_store.launcher_path(&runtime).display()
            );
            Ok(())
        }
        _ => Err("usage: dart runtimes [list | download [minecraft-version]]".into()),
    }
}

fn parse_runtime(value: &str) -> Result<FabricRuntime, Box<dyn Error>> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("runtime must be <minecraft>/<loader>/<installer>".into());
    }
    Ok(FabricRuntime::new(parts[0], parts[1], parts[2])?)
}

fn print_help() {
    println!(
        "Dart manages local Fabric server instances.\n\n\
         Usage:\n  \
         dart [--home <directory>]                           Open the TUI\n  \
         dart [--home <directory>] list                      List instances\n  \
         dart [--home <directory>] create <id> <name...>\n  \
              [--minecraft <version> | --runtime <mc/loader/installer>] [--accept-eula]\n  \
         dart [--home <directory>] runtimes list             List cached launchers\n  \
         dart [--home <directory>] runtimes download [mc]    Cache a launcher\n  \
         dart help                                           Show this help\n\n\
         Create downloads the newest stable Fabric runtime by default.\n\
         DART_HOME overrides the default data directory."
    );
}
