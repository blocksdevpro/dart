//! The Dart daemon background service binary (`dartd`).
//!
//! Exposes a dual-transport HTTP REST and WebSocket API over both a local
//! Unix domain socket (for CLI/TUI) and an optional TCP listener (for Web/Desktop Panel).

use dart_daemon::{Daemon, DartPaths};
use std::env;
use std::error::Error;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

const HELP: &str = "\
dartd: The Dart Minecraft server manager daemon

Usage:
  dartd [OPTIONS]

Options:
  --home <dir>     Override root data directory ($DART_HOME)
  --port <port>    TCP listener port (default: 4545)
  --host <host>    TCP listener host (default: 127.0.0.1)
  --no-tcp         Disable TCP listener (listen on Unix socket only)
  --no-socket      Disable Unix domain socket (listen on TCP only)
  -h, --help       Show this help message
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{HELP}");
        return Ok(());
    }

    let mut home = None;
    let mut host = "127.0.0.1".to_owned();
    let mut port = 4545u16;
    let mut enable_tcp = true;
    let mut enable_socket = true;

    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--home" => {
                if let Some(val) = iter.next() {
                    home = Some(PathBuf::from(val));
                }
            }
            "--host" => {
                if let Some(val) = iter.next() {
                    host = val;
                }
            }
            "--port" => {
                if let Some(val) = iter.next() {
                    port = val.parse()?;
                }
            }
            "--no-tcp" => {
                enable_tcp = false;
            }
            "--no-socket" => {
                enable_socket = false;
            }
            _ => {}
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

    println!("====================================================");
    println!("  dartd: Dart Server Daemon v{}", env!("CARGO_PKG_VERSION"));
    println!("====================================================");
    println!("  Data Home:   {}", paths.home().display());

    let socket_path = if enable_socket {
        let sock = paths.socket_path();
        println!("  Unix Socket: {}", sock.display());
        Some(sock)
    } else {
        None
    };

    let tcp_addr = if enable_tcp {
        let addr_str = format!("{host}:{port}");
        let addr: SocketAddr = addr_str.parse()?;
        println!("  TCP Server:  http://{addr}");
        Some(addr)
    } else {
        None
    };
    println!("----------------------------------------------------");
    println!("Press Ctrl+C to terminate the daemon.\n");

    // Event logger
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

    let srv_daemon = daemon.clone();
    let srv_socket = socket_path.clone();
    tokio::select! {
        srv_res = dart_server::serve(srv_daemon, srv_socket, tcp_addr) => {
            if let Err(err) = srv_res {
                eprintln!("[dartd error] API server error: {err}");
            }
            println!("\n[dartd] API shutdown requested.");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\n[dartd] Received SIGINT signal.");
        }
    }

    println!("Stopping running instances...");
    let _ = daemon.stop_all().await;

    // Cleanup socket file
    if let Some(sock) = socket_path {
        if sock.exists() {
            let _ = fs::remove_file(sock);
        }
    }

    println!("[dartd] Shutdown complete.");
    Ok(())
}
