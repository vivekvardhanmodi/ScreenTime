use async_trait::async_trait;
use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use screentime_core::ipc::{socket_name_from_path, BrowserIpcMessage, UrlUpdate};
use screentime_core::{
    DbActorHandle, IdleDetector, IdleState, TrackerError, WindowInfo, WindowTracker,
};
use screentimed::{IpcServer, IpcServerError, SessionManager};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[derive(Clone)]
struct SimpleWindowTracker {
    initial: Option<WindowInfo>,
}

#[async_trait]
impl WindowTracker for SimpleWindowTracker {
    async fn start(&mut self, _tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        Ok(())
    }
    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        Ok(self.initial.clone())
    }
    async fn stop(&mut self) -> Result<(), TrackerError> {
        Ok(())
    }
}

#[derive(Clone)]
struct SimpleIdleDetector;

#[async_trait]
impl IdleDetector for SimpleIdleDetector {
    async fn start(&mut self, _tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        Ok(())
    }
    fn is_idle(&self) -> bool {
        false
    }
    async fn stop(&mut self) -> Result<(), TrackerError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_ipc_single_instance_enforcement() {
    let dir = tempdir().expect("Failed to create tempdir");
    let sock_path = dir.path().join("test_single_instance.sock");

    let (url_tx1, mut _url_rx1) = mpsc::channel(16);
    let mut server1 = IpcServer::start_with_path(&sock_path, url_tx1)
        .await
        .expect("Server 1 should bind successfully");

    let (url_tx2, mut _url_rx2) = mpsc::channel(16);
    let server2_res = IpcServer::start_with_path(&sock_path, url_tx2).await;
    assert!(
        matches!(server2_res, Err(IpcServerError::AlreadyRunning)),
        "Second server on same path must fail with AlreadyRunning"
    );

    server1.shutdown().await;

    let (url_tx3, mut _url_rx3) = mpsc::channel(16);
    let mut server3 = IpcServer::start_with_path(&sock_path, url_tx3)
        .await
        .expect("Server 3 should bind successfully after server 1 shutdown");

    server3.shutdown().await;
}

#[tokio::test]
async fn test_ipc_stale_socket_recovery() {
    let dir = tempdir().expect("Failed to create tempdir");
    let sock_path = dir.path().join("test_stale.sock");

    std::fs::write(&sock_path, b"stale socket").expect("Write stale file");
    assert!(sock_path.exists());

    let (url_tx, mut _url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&sock_path, url_tx)
        .await
        .expect("Server should bind and overwrite stale socket file");

    server.shutdown().await;
}

#[tokio::test]
async fn test_ipc_client_url_streaming_and_disconnection() {
    let dir = tempdir().expect("Failed to create tempdir");
    let sock_path = dir.path().join("test_stream.sock");

    let (url_tx, mut url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&sock_path, url_tx)
        .await
        .expect("Server bind");

    let sock_name = socket_name_from_path(&sock_path).expect("Socket name");
    let mut client = Stream::connect(sock_name)
        .await
        .expect("Client connect to server");

    let msg1 = UrlUpdate::new(
        "chrome",
        Some("https://doc.rust-lang.org".to_string()),
        Some("Rust Docs".to_string()),
    );
    let line1 = format!("{}\n", serde_json::to_string(&msg1).unwrap());
    client
        .write_all(line1.as_bytes())
        .await
        .expect("Write msg1");

    let received1 = tokio::time::timeout(Duration::from_millis(500), url_rx.recv())
        .await
        .expect("Timeout receiving msg1")
        .expect("Channel closed");
    assert_eq!(received1.browser_type, "chrome");
    assert_eq!(received1.url.as_deref(), Some("https://doc.rust-lang.org"));
    assert_eq!(received1.title.as_deref(), Some("Rust Docs"));

    let msg2 = BrowserIpcMessage::UrlUpdate {
        browser_type: "firefox".to_string(),
        url: Some("https://crates.io".to_string()),
        title: Some("Crates.io".to_string()),
        timestamp: Some(100.0),
    };
    let line2 = format!("{}\n", serde_json::to_string(&msg2).unwrap());
    client
        .write_all(line2.as_bytes())
        .await
        .expect("Write msg2");

    let received2 = tokio::time::timeout(Duration::from_millis(500), url_rx.recv())
        .await
        .expect("Timeout receiving msg2")
        .expect("Channel closed");
    assert_eq!(received2.browser_type, "firefox");
    assert_eq!(received2.url.as_deref(), Some("https://crates.io"));

    drop(client);

    server.shutdown().await;
}

#[tokio::test]
async fn test_ipc_wired_into_session_manager() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_ipc_session.db");
    let sock_path = dir.path().join("test_ipc_session.sock");

    let db = DbActorHandle::spawn(&db_path).expect("Spawn DbActor");

    let (url_tx, url_rx) = mpsc::channel(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let mut ipc_server = IpcServer::start_with_path(&sock_path, url_tx)
        .await
        .expect("IPC server bind");

    let browser_window = WindowInfo::new("google-chrome", "Google Chrome");
    let tracker = SimpleWindowTracker {
        initial: Some(browser_window),
    };
    let idle = SimpleIdleDetector;

    let sm = SessionManager::new(db.clone());
    let runner = tokio::spawn(async move { sm.run_loop(tracker, idle, url_rx, shutdown_rx).await });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let sock_name = socket_name_from_path(&sock_path).expect("Socket name");
    let mut client = Stream::connect(sock_name).await.expect("Client connect");

    let initial_update = UrlUpdate::new(
        "chrome",
        Some("https://news.ycombinator.com".to_string()),
        Some("Hacker News".to_string()),
    );
    let payload = format!("{}\n", serde_json::to_string(&initial_update).unwrap());
    client.write_all(payload.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let active_sessions = db.get_sessions_in_range(0.0, 2e9).await.unwrap();
    assert_eq!(active_sessions.len(), 1);
    assert_eq!(active_sessions[0].app_class, "google-chrome");
    assert_eq!(
        active_sessions[0].website_url.as_deref(),
        Some("https://news.ycombinator.com")
    );

    tokio::time::sleep(Duration::from_millis(2100)).await;

    let next_update = UrlUpdate::new(
        "chrome",
        Some("https://github.com".to_string()),
        Some("GitHub".to_string()),
    );
    let payload2 = format!("{}\n", serde_json::to_string(&next_update).unwrap());
    client.write_all(payload2.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(1100)).await;

    shutdown_tx.send(()).await.unwrap();
    ipc_server.shutdown().await;

    let _ = runner.await.unwrap();

    let all_sessions = db.get_sessions_in_range(0.0, 2e9).await.unwrap();
    assert_eq!(all_sessions.len(), 2);
    assert_eq!(
        all_sessions[0].website_url.as_deref(),
        Some("https://news.ycombinator.com")
    );
    assert_eq!(
        all_sessions[1].website_url.as_deref(),
        Some("https://github.com")
    );
}
