//! The Dart daemon background process binary (`dartd`).
//!
//! `dartd` is the background control layer that manages local Minecraft
//! server processes, monitors instance lifecycles, and executes operations.

use dart_daemon::{Daemon, DartPaths};
use std::env;
use std::error::Error;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("dartd: The Dart server daemon\n\nUsage: dartd [--home <directory>]");
        return Ok(());
    }

    let mut home = None;
    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--home" {
            if let Some(val) = iter.next() {
                home = Some(PathBuf::from(val));
            }
        }
    }

    let home_path = home
        .or_else(|| env::var_os("DART_HOME").map(PathBuf::from))
        .or_else(|| {
            env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .map(|p| p.join("dart"))
        })
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".local/share/dart"))
        })
        .unwrap_or_else(|| PathBuf::from(".dart"));

    let paths = DartPaths::new(home_path);
    let (daemon, mut events) = Daemon::new(paths.clone())?;

    println!("dartd running for data home: {}", paths.home().display());
    println!("Press Ctrl+C to terminate the daemon.");

    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event {
                dart_daemon::ServerEvent::StateChanged { id, state } => {
                    eprintln!("[dartd] instance '{id}' state changed: {state:?}");
                }
                dart_daemon::ServerEvent::OperationFailed { id, message } => {
                    eprintln!("[dartd] instance '{id}' error: {message}");
                }
                _ => {}
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    println!("\n[dartd] Received shutdown signal. Stopping running instances...");
    let _ = daemon.stop_all().await;
    println!("[dartd] Shutdown complete.");

    Ok(())
}
