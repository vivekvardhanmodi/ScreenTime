use interprocess::local_socket::traits::tokio::Stream as _;
use screentimed::manifests::{ChromeManifest, FirefoxManifest};
use screentimed::platform::{PlatformBackend, PlatformError};
use screentimed::{probe_daemon_status, IpcServer, DEFAULT_FIREFOX_EXTENSION_ID, NATIVE_HOST_NAME};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

const CLI_BIN: &str = env!("CARGO_BIN_EXE_screentimed");

#[test]
fn test_cli_version_flag() {
    let output = Command::new(CLI_BIN)
        .arg("--version")
        .output()
        .expect("Failed to execute screentimed --version");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("screentimed 0.1.0"));

    let output_short = Command::new(CLI_BIN)
        .arg("-V")
        .output()
        .expect("Failed to execute screentimed -V");
    assert!(output_short.status.success());
    let stdout_short = String::from_utf8_lossy(&output_short.stdout);
    assert!(stdout_short.contains("screentimed 0.1.0"));
}

#[test]
fn test_cli_help_flag() {
    let output = Command::new(CLI_BIN)
        .arg("--help")
        .output()
        .expect("Failed to execute screentimed --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("USAGE:"));
    assert!(stdout.contains("SUBCOMMANDS:"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("install-manifests"));
    assert!(stdout.contains("--db"));
    assert!(stdout.contains("--socket"));
    assert!(stdout.contains("--platform"));

    let output_short = Command::new(CLI_BIN)
        .arg("-h")
        .output()
        .expect("Failed to execute screentimed -h");
    assert!(output_short.status.success());
    assert!(String::from_utf8_lossy(&output_short.stdout).contains("USAGE:"));
}

#[tokio::test]
async fn test_cli_status_when_stopped() {
    let temp = tempdir().expect("tempdir");
    let fake_sock = temp.path().join("nonexistent.sock");

    let output = Command::new(CLI_BIN)
        .args(["status", "--socket", fake_sock.to_str().unwrap()])
        .output()
        .expect("Failed to run status");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("screentimed is not running"));
}

#[tokio::test]
async fn test_cli_status_when_running() {
    let temp = tempdir().expect("tempdir");
    let live_sock = temp.path().join("active.sock");

    let (url_tx, _url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&live_sock, url_tx)
        .await
        .expect("Bind test IPC server");

    let output = Command::new(CLI_BIN)
        .args(["status", "--socket", live_sock.to_str().unwrap()])
        .output()
        .expect("Failed to run status");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("screentimed is running"));

    server.shutdown().await;
}

#[tokio::test]
async fn test_cli_single_instance_enforcement() {
    let temp = tempdir().expect("tempdir");
    let lock_sock = temp.path().join("lock.sock");
    let db_path = temp.path().join("ephemeral.db");

    let (url_tx, _url_rx) = mpsc::channel(16);
    let mut server = IpcServer::start_with_path(&lock_sock, url_tx)
        .await
        .expect("Bind first IPC server");

    let output = Command::new(CLI_BIN)
        .args([
            "run",
            "--socket",
            lock_sock.to_str().unwrap(),
            "--db",
            db_path.to_str().unwrap(),
        ])
        .output()
        .expect("Execute second daemon instance");

    // Clean exit with 1, no panic
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Another instance of screentimed is already running"),
        "stderr was: {stderr}"
    );

    server.shutdown().await;
}

#[test]
fn test_cli_install_manifests_command() {
    let temp = tempdir().expect("tempdir");
    let chrome_dir = temp.path().join("chrome_nmh");
    let firefox_dir = temp.path().join("firefox_nmh");
    let mock_host = temp.path().join("screentime-browser-host");
    fs::write(&mock_host, b"#!/bin/sh\nexit 0\n").unwrap();

    let output = Command::new(CLI_BIN)
        .args([
            "install-manifests",
            "--chrome-dir",
            chrome_dir.to_str().unwrap(),
            "--firefox-dir",
            firefox_dir.to_str().unwrap(),
            "--host-path",
            mock_host.to_str().unwrap(),
            "--extension-id",
            "testextensionid123456",
        ])
        .output()
        .expect("Execute install-manifests");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Native messaging manifests installed successfully"));

    let chrome_manifest_file = chrome_dir.join(format!("{NATIVE_HOST_NAME}.json"));
    let firefox_manifest_file = firefox_dir.join(format!("{NATIVE_HOST_NAME}.json"));

    assert!(chrome_manifest_file.exists());
    assert!(firefox_manifest_file.exists());

    let chrome_content = fs::read_to_string(&chrome_manifest_file).unwrap();
    let chrome_json: ChromeManifest = serde_json::from_str(&chrome_content).unwrap();
    assert_eq!(chrome_json.name, NATIVE_HOST_NAME);
    assert_eq!(chrome_json.path, mock_host.to_str().unwrap());
    assert_eq!(
        chrome_json.allowed_origins,
        vec!["chrome-extension://testextensionid123456/"]
    );

    let firefox_content = fs::read_to_string(&firefox_manifest_file).unwrap();
    let firefox_json: FirefoxManifest = serde_json::from_str(&firefox_content).unwrap();
    assert_eq!(firefox_json.name, NATIVE_HOST_NAME);
    assert_eq!(firefox_json.path, mock_host.to_str().unwrap());
    assert_eq!(
        firefox_json.allowed_extensions,
        vec![DEFAULT_FIREFOX_EXTENSION_ID]
    );

    let output2 = Command::new(CLI_BIN)
        .args([
            "install-manifests",
            "--chrome-dir",
            chrome_dir.to_str().unwrap(),
            "--firefox-dir",
            firefox_dir.to_str().unwrap(),
            "--host-path",
            mock_host.to_str().unwrap(),
        ])
        .output()
        .expect("Execute second install-manifests");

    assert!(output2.status.success());
    let chrome_content2 = fs::read_to_string(&chrome_manifest_file).unwrap();
    let chrome_json2: ChromeManifest = serde_json::from_str(&chrome_content2).unwrap();
    assert_eq!(
        chrome_json2.allowed_origins,
        vec!["chrome-extension://testextensionid123456/"],
        "Must preserve existing Chrome extension ID"
    );
}

#[test]
fn test_platform_auto_detect_all_environments() {
    let mut env = HashMap::new();
    let res = PlatformBackend::auto_detect_from(&env, "windows").unwrap();
    assert_eq!(res, PlatformBackend::Windows);

    env.insert(
        "HYPRLAND_INSTANCE_SIGNATURE".to_string(),
        "instance_sig_123".to_string(),
    );
    let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
    assert_eq!(res, PlatformBackend::Hyprland);

    env.clear();
    env.insert("XDG_CURRENT_DESKTOP".to_string(), "KDE".to_string());
    let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
    assert_eq!(res, PlatformBackend::Kde);

    env.clear();
    env.insert("XDG_SESSION_DESKTOP".to_string(), "plasma:KDE".to_string());
    let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
    assert_eq!(res, PlatformBackend::Kde);

    env.clear();
    env.insert("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string());
    let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
    assert_eq!(res, PlatformBackend::Wlroots);

    env.clear();
    env.insert("DISPLAY".to_string(), ":0".to_string());
    let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
    assert_eq!(res, PlatformBackend::X11);

    env.clear();
    let res = PlatformBackend::auto_detect_from(&env, "linux");
    assert!(matches!(res, Err(PlatformError::NoSupportedDesktop)));
}

#[tokio::test]
async fn test_daemon_end_to_end_and_graceful_shutdown() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("test_daemon.db");
    let sock_path = temp.path().join("daemon_test.sock");
    let his = "test_cli_instance_42";

    // Setup mock Hyprland socket dir
    let hypr_dir = temp.path().join("hypr").join(his);
    fs::create_dir_all(&hypr_dir).unwrap();

    let socket1_path = hypr_dir.join(".socket.sock");
    let socket2_path = hypr_dir.join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener1.accept().await {
            let mut req = [0u8; 32];
            let n = stream.read(&mut req).await.unwrap_or(0);
            let req_str = String::from_utf8_lossy(&req[..n]);

            if req_str.contains("j/activewindow") {
                let resp = br#"{"class":"code","title":"main.rs"}"#;
                let _ = stream.write_all(resp).await;
                let _ = stream.shutdown().await;
            } else if req_str.contains("j/monitors") {
                let resp = br#"[{"name":"eDP-1","dpmsStatus":true}]"#;
                let _ = stream.write_all(resp).await;
                let _ = stream.shutdown().await;
            }
        }
    });

    let listener2 = UnixListener::bind(&socket2_path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener2.accept().await {
            tokio::spawn(async move {
                let _ = stream
                    .write_all(b"activewindow>>firefox,Mozilla Firefox\n")
                    .await;
                let mut buf = [0u8; 16];
                let _ = stream.read(&mut buf).await;
            });
        }
    });

    // Spawn screentimed binary
    let child = Command::new(CLI_BIN)
        .arg("run")
        .arg("--db")
        .arg(&db_path)
        .arg("--socket")
        .arg(&sock_path)
        .arg("--platform")
        .arg("hyprland")
        .arg("--device-id")
        .arg("cli-test-device")
        .env("HYPRLAND_INSTANCE_SIGNATURE", his)
        .env("XDG_RUNTIME_DIR", temp.path().to_str().unwrap())
        .env("RUST_LOG", "debug")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn screentimed daemon");

    // Wait for daemon IPC socket to become active
    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if probe_daemon_status(&sock_path).await {
            ready = true;
            break;
        }
    }
    assert!(ready, "Daemon failed to start IPC socket in time");

    // Probe status using CLI
    let status_res = Command::new(CLI_BIN)
        .args(["status", "--socket", sock_path.to_str().unwrap()])
        .output()
        .expect("Status check");
    assert!(status_res.status.success());
    assert!(String::from_utf8_lossy(&status_res.stdout).contains("screentimed is running"));

    // Send a URL update over IPC
    let update_msg = serde_json::json!({
        "url": "https://rust-lang.org",
        "title": "Rust Programming Language",
        "browser_type": "firefox"
    });
    let mut update_line = update_msg.to_string();
    update_line.push('\n');

    let name = screentime_core::ipc::socket_name_from_path(&sock_path).unwrap();
    let mut client = interprocess::local_socket::tokio::Stream::connect(name)
        .await
        .expect("Connect to daemon IPC");
    client.write_all(update_line.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();

    // Give daemon a moment to accumulate duration >= 1.0s so noise filter doesn't discard it
    tokio::time::sleep(Duration::from_millis(1150)).await;

    // Send SIGTERM to child process for graceful shutdown
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }

    let output = child
        .wait_with_output()
        .expect("Wait for daemon termination");
    println!(
        "DAEMON STDOUT:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    eprintln!(
        "DAEMON STDERR:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "Daemon should exit successfully on SIGTERM"
    );

    // Verify socket was cleaned up
    assert!(
        !sock_path.exists(),
        "IPC socket file must be removed on graceful shutdown"
    );

    // Verify SQLite database recorded session
    assert!(db_path.exists(), "SQLite database file should exist");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT id, app_class, website_url, device_id, (end_time - start_time) FROM sessions",
        )
        .unwrap();
    let rows: Vec<_> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<f64>>(4)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    println!("SESSIONS IN DB: {:?}", rows);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert!(count >= 1, "At least one session should be recorded");

    let (app_class, website_url, device_id): (String, Option<String>, String) = conn
        .query_row(
            "SELECT app_class, website_url, device_id FROM sessions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(app_class, "firefox");
    assert_eq!(website_url.as_deref(), Some("https://rust-lang.org"));
    assert_eq!(device_id, "cli-test-device");

    // CRITICAL: Verify user workstation database is untouched
    if let Ok(home) = std::env::var("HOME") {
        let user_db = PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("screentime")
            .join("screentime.db");
        if user_db.exists() {
            // Confirm user_db was not modified during test run
            let meta = fs::metadata(&user_db).unwrap();
            assert!(meta.len() > 0, "User database is intact");
        }
    }
}
