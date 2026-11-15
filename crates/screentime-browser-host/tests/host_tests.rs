use screentime_browser_host::run_host_with_path;
use screentime_core::ipc::{
    read_length_prefixed, write_length_prefixed, BrowserExtensionMessage, HostExtensionResponse,
};
use screentimed::IpcServer;
use std::io::Cursor;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_browser_host_in_process_framing_and_ipc_dispatch() {
    let dir = tempdir().expect("tempdir");
    let sock_path = dir.path().join("test_host.sock");

    let (url_tx, mut url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&sock_path, url_tx)
        .await
        .expect("Server bind");

    let mut stdin_buffer = Vec::new();

    write_length_prefixed(&mut stdin_buffer, &BrowserExtensionMessage::Ping).unwrap();

    let update = BrowserExtensionMessage::UrlUpdate {
        browser_type: "chrome".to_string(),
        url: Some("https://example.com/page".to_string()),
        title: Some("Example Page".to_string()),
    };
    write_length_prefixed(&mut stdin_buffer, &update).unwrap();

    let raw_unknown = serde_json::json!({
        "type": "custom_unsupported"
    });
    write_length_prefixed(&mut stdin_buffer, &raw_unknown).unwrap();

    let stdin_cursor = Cursor::new(stdin_buffer);
    let stdout_cursor = Cursor::new(Vec::new());

    let (host_res, mut stdout_cursor) =
        run_host_with_path(stdin_cursor, stdout_cursor, &sock_path).await;
    assert!(host_res.is_ok());

    let received = tokio::time::timeout(Duration::from_millis(500), url_rx.recv())
        .await
        .expect("Timeout waiting for IPC dispatch")
        .expect("Channel closed");
    assert_eq!(received.browser_type, "chrome");
    assert_eq!(received.url.as_deref(), Some("https://example.com/page"));
    assert_eq!(received.title.as_deref(), Some("Example Page"));

    stdout_cursor.set_position(0);

    let resp1: HostExtensionResponse = read_length_prefixed(&mut stdout_cursor)
        .unwrap()
        .expect("resp1");
    assert_eq!(resp1, HostExtensionResponse::Pong);

    let resp2: HostExtensionResponse = read_length_prefixed(&mut stdout_cursor)
        .unwrap()
        .expect("resp2");
    assert_eq!(resp2, HostExtensionResponse::Ok);

    let resp3: HostExtensionResponse = read_length_prefixed(&mut stdout_cursor)
        .unwrap()
        .expect("resp3");
    assert!(matches!(resp3, HostExtensionResponse::Error { .. }));

    let resp4: Option<HostExtensionResponse> = read_length_prefixed(&mut stdout_cursor).unwrap();
    assert_eq!(resp4, None);

    server.shutdown().await;
}

#[tokio::test]
async fn test_browser_host_resilience_when_daemon_down() {
    let dir = tempdir().expect("tempdir");
    let non_existent_sock = dir.path().join("does_not_exist.sock");

    let mut stdin_buffer = Vec::new();
    let update = BrowserExtensionMessage::UrlUpdate {
        browser_type: "firefox".to_string(),
        url: Some("https://example.com".to_string()),
        title: Some("Example".to_string()),
    };
    write_length_prefixed(&mut stdin_buffer, &update).unwrap();
    write_length_prefixed(&mut stdin_buffer, &BrowserExtensionMessage::Ping).unwrap();

    let stdout_cursor = Cursor::new(Vec::new());

    let (host_res, mut stdout_cursor) =
        run_host_with_path(Cursor::new(stdin_buffer), stdout_cursor, &non_existent_sock).await;
    assert!(
        host_res.is_ok(),
        "Host should not fail or panic when daemon is unreachable"
    );

    stdout_cursor.set_position(0);
    let resp1: HostExtensionResponse = read_length_prefixed(&mut stdout_cursor)
        .unwrap()
        .expect("resp1");
    assert_eq!(resp1, HostExtensionResponse::Ok);

    let resp2: HostExtensionResponse = read_length_prefixed(&mut stdout_cursor)
        .unwrap()
        .expect("resp2");
    assert_eq!(resp2, HostExtensionResponse::Pong);
}

#[tokio::test]
async fn test_browser_host_binary_process_execution() {
    let dir = tempdir().expect("tempdir");
    let sock_path = dir.path().join("test_bin_host.sock");

    let (url_tx, mut url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&sock_path, url_tx)
        .await
        .expect("Server bind");

    let bin_path = env!("CARGO_BIN_EXE_screentime-browser-host");

    let mut child = Command::new(bin_path)
        .env("SCREENTIME_SOCKET", sock_path.to_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to spawn screentime-browser-host binary");

    let mut child_stdin = child.stdin.take().expect("Child stdin");
    let mut child_stdout = child.stdout.take().expect("Child stdout");

    let update = BrowserExtensionMessage::UrlUpdate {
        browser_type: "chrome".to_string(),
        url: Some("https://kernel.org".to_string()),
        title: Some("Linux Kernel".to_string()),
    };
    write_length_prefixed(&mut child_stdin, &update).expect("Write to child stdin");

    let resp: HostExtensionResponse = read_length_prefixed(&mut child_stdout)
        .expect("Read from child stdout")
        .expect("Expected response");
    assert_eq!(resp, HostExtensionResponse::Ok);

    let received = tokio::time::timeout(Duration::from_millis(1000), url_rx.recv())
        .await
        .expect("Timeout receiving URL from child process")
        .expect("Channel closed");
    assert_eq!(received.browser_type, "chrome");
    assert_eq!(received.url.as_deref(), Some("https://kernel.org"));

    drop(child_stdin);

    let status = child.wait().expect("Failed to wait on child process");
    assert!(
        status.success(),
        "Process must exit successfully on stdin EOF without zombie"
    );

    server.shutdown().await;
}
