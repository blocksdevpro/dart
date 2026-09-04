use dart_daemon::{Daemon, DartPaths, FabricRuntime};
use dart_protocol::instance::{
    CreateInstanceRequest, InstanceDto, InstanceSizeDto, InstanceStateDto,
};
use dart_protocol::system::{HealthResponse, SystemInfoResponse};
use dart_server::{AppState, build_router};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixStream};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("dart-server-test-{name}-{nanos}"));
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
async fn http_endpoints_over_tcp() {
    let test_dir = TestDir::new("tcp");
    let paths = DartPaths::new(test_dir.path().clone());
    let (daemon, _events) = Daemon::new(paths).unwrap();

    // Seed cached runtime
    let runtime = FabricRuntime::new("1.21.4", "0.16.10", "1.0.1").unwrap();
    daemon
        .runtime_store()
        .install_bytes(&runtime, b"PK\x03\x04fixture")
        .unwrap();

    let state = AppState::new(daemon);
    let router = build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://{addr}");

    // 1. Health check
    let health: HealthResponse = client
        .get(format!("{base_url}/api/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health.status, "ok");

    // 2. System info
    let info: SystemInfoResponse = client
        .get(format!("{base_url}/api/v1/system"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info.instances_count, 0);
    assert_eq!(info.runtimes_count, 1);

    // 3. Create instance
    let create_req = CreateInstanceRequest {
        name: "Survival SMP".to_owned(),
        minecraft: Some("1.21.4".to_owned()),
        loader: Some("0.16.10".to_owned()),
        installer: Some("1.0.1".to_owned()),
        accept_eula: true,
        size: InstanceSizeDto::Friends,
    };

    let created: InstanceDto = client
        .post(format!("{base_url}/api/v1/instances"))
        .json(&create_req)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created.id, "survival-smp");
    assert_eq!(created.name, "Survival SMP");
    assert_eq!(created.config.launch.max_memory_mib, 4096);
    assert_eq!(created.state, InstanceStateDto::Stopped);

    // 4. List instances
    let list: Vec<InstanceDto> = client
        .get(format!("{base_url}/api/v1/instances"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "survival-smp");

    // 5. Get instance state
    let state_dto: InstanceStateDto = client
        .get(format!("{base_url}/api/v1/instances/survival-smp/state"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state_dto, InstanceStateDto::Stopped);
}

#[tokio::test]
async fn http_over_unix_domain_socket() {
    let test_dir = TestDir::new("unix-sock");
    let socket_path = test_dir.path().join("dartd.sock");
    let paths = DartPaths::new(test_dir.path().clone());
    let (daemon, _events) = Daemon::new(paths).unwrap();

    let state = AppState::new(daemon);
    let router = build_router(state);

    let sock_clone = socket_path.clone();
    tokio::spawn(async move {
        dart_server::serve_unix(router, sock_clone).await.unwrap();
    });

    // Wait briefly for socket creation
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(socket_path.exists());

    // Connect via UnixStream and send raw HTTP request
    let mut stream = UnixStream::connect(&socket_path).await.unwrap();
    let request = "GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains(r#""status":"ok""#));
}
