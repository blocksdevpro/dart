use dart_client::DartClient;
use dart_daemon::{Daemon, DartPaths, FabricRuntime};
use dart_protocol::instance::CreateInstanceRequest;
use dart_server::{build_router, AppState};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("dart-client-test-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn client_communicates_over_unix_domain_socket() {
    let test_dir = TestDir::new("client-uds");
    let socket_path = test_dir.path().join("dartd.sock");
    let paths = DartPaths::new(test_dir.path().clone());
    let (daemon, _events) = Daemon::new(paths).unwrap();

    // Cache a runtime
    let runtime = FabricRuntime::new("1.21.4", "0.16.10", "1.0.1").unwrap();
    daemon
        .runtime_store()
        .install_bytes(&runtime, b"PK\x03\x04test-jar")
        .unwrap();

    let state = AppState::new(daemon);
    let router = build_router(state);

    let sock_clone = socket_path.clone();
    tokio::spawn(async move {
        dart_server::serve_unix(router, sock_clone).await.unwrap();
    });

    // Wait for socket to appear
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(socket_path.exists());

    let client = DartClient::unix(&socket_path);

    // 1. Health check
    let health = client.health().await.unwrap();
    assert_eq!(health.status, "ok");

    // 2. System info
    let info = client.system_info().await.unwrap();
    assert_eq!(info.instances_count, 0);
    assert_eq!(info.runtimes_count, 1);

    // 3. Create instance
    let req = CreateInstanceRequest {
        id: "vanilla".to_owned(),
        name: "Vanilla Server".to_owned(),
        minecraft: Some("1.21.4".to_owned()),
        loader: Some("0.16.10".to_owned()),
        installer: Some("1.0.1".to_owned()),
        accept_eula: true,
        min_memory_mib: Some(1024),
        max_memory_mib: Some(2048),
        java: None,
    };
    let created = client.create_instance(&req).await.unwrap();
    assert_eq!(created.id, "vanilla");
    assert_eq!(created.name, "Vanilla Server");

    // 4. List instances
    let list = client.list_instances().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "vanilla");

    // 5. Get instance
    let fetched = client.get_instance("vanilla").await.unwrap();
    assert_eq!(fetched.id, "vanilla");

    // 6. Get state
    let state = client.get_state("vanilla").await.unwrap();
    assert_eq!(state, dart_protocol::instance::InstanceStateDto::Stopped);

    // 7. Interactive WebSocket console attach
    let mut console = client
        .attach_console_unix(&socket_path, "vanilla")
        .await
        .unwrap();

    // First message should be History
    let history_msg = console.recv().await.unwrap();
    assert!(matches!(
        history_msg,
        Some(dart_protocol::console::ConsoleWsServerMessage::History { .. })
    ));

    // Send a ping and expect pong or send a command
    console.ping().await.unwrap();
}
