use screentime_core::{DbActorHandle, IdleDetector, IdleState, WindowInfo, WindowTracker};
use screentimed::hyprland::{
    parse_active_window_event, parse_active_window_json, parse_active_window_line,
    parse_dpms_event, parse_dpms_line, parse_dpms_payload, parse_monitors_json, resolve_socket_dir,
    HyprlandIdleDetector, HyprlandWindowTracker,
};
use screentimed::SessionManager;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

#[test]
fn test_parse_active_window_event_simple() {
    let win = parse_active_window_event("kitty,fish").expect("parsed window");
    assert_eq!(win.app_class, "kitty");
    assert_eq!(win.title, "fish");
}

#[test]
fn test_parse_active_window_event_titles_with_commas() {
    let win = parse_active_window_event("code,main.rs - screentime, edited").expect("parsed");
    assert_eq!(win.app_class, "code");
    assert_eq!(win.title, "main.rs - screentime, edited");

    let win2 = parse_active_window_event("firefox,Issue #42: comma, test, title - Mozilla Firefox")
        .expect("parsed");
    assert_eq!(win2.app_class, "firefox");
    assert_eq!(
        win2.title,
        "Issue #42: comma, test, title - Mozilla Firefox"
    );
}

#[test]
fn test_parse_active_window_event_unfocused_and_empty() {
    assert_eq!(parse_active_window_event(""), None);
    assert_eq!(parse_active_window_event("   "), None);
    assert_eq!(parse_active_window_event(","), None);
    assert_eq!(parse_active_window_event(" , "), None);
    assert_eq!(parse_active_window_event(",some title"), None);
    assert_eq!(parse_active_window_event("  , some title"), None);

    let no_comma = parse_active_window_event("alacritty").expect("parsed class without comma");
    assert_eq!(no_comma.app_class, "alacritty");
    assert_eq!(no_comma.title, "");
}

#[test]
fn test_parse_active_window_line() {
    assert_eq!(parse_active_window_line("workspace>>2"), None);
    assert_eq!(parse_active_window_line("focusedmon>>DP-1,1"), None);
    assert_eq!(parse_active_window_line("openwindow>>0x123"), None);

    let focused = parse_active_window_line("activewindow>>kitty,fish");
    assert_eq!(focused, Some(Some(WindowInfo::new("kitty", "fish"))));

    let unfocused = parse_active_window_line("activewindow>>,");
    assert_eq!(unfocused, Some(None));

    let unfocused_empty = parse_active_window_line("activewindow>>");
    assert_eq!(unfocused_empty, Some(None));
}

#[test]
fn test_parse_active_window_json() {
    let json = br#"{"class":"firefox","title":"GitHub - PR #1, test"}"#;
    let win = parse_active_window_json(json).unwrap().expect("parsed");
    assert_eq!(win.app_class, "firefox");
    assert_eq!(win.title, "GitHub - PR #1, test");

    assert_eq!(parse_active_window_json(b"").unwrap(), None);
    assert_eq!(parse_active_window_json(b"{}").unwrap(), None);
    assert_eq!(
        parse_active_window_json(br#"{"class":"","title":"foo"}"#).unwrap(),
        None
    );
    assert_eq!(
        parse_active_window_json(br#"{"class":"   ","title":"foo"}"#).unwrap(),
        None
    );
    assert_eq!(
        parse_active_window_json(br#"{"title":"only title"}"#).unwrap(),
        None
    );
}

#[test]
fn test_parse_dpms_event_and_line() {
    assert_eq!(parse_dpms_payload("0"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_payload("off"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_payload("false"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_payload("1"), Some(IdleState::Active));
    assert_eq!(parse_dpms_payload("on"), Some(IdleState::Active));
    assert_eq!(parse_dpms_payload("true"), Some(IdleState::Active));

    assert_eq!(parse_dpms_event("DP-1,0"), Some((Some("DP-1".into()), false)));
    assert_eq!(
        parse_dpms_event("HDMI-A-1,on"),
        Some((Some("HDMI-A-1".into()), true))
    );
    assert_eq!(parse_dpms_event("0"), Some((None, false)));

    assert_eq!(parse_dpms_payload("DP-1,0"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_payload("HDMI-A-1,off"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_payload("DP-1,1"), Some(IdleState::Active));
    assert_eq!(parse_dpms_payload("HDMI-A-1,on"), Some(IdleState::Active));
    assert_eq!(parse_dpms_payload("unknown"), None);

    assert_eq!(parse_dpms_line("dpms>>0"), Some(IdleState::Idle));
    assert_eq!(parse_dpms_line("dpms>>1"), Some(IdleState::Active));
    assert_eq!(parse_dpms_line("activewindow>>kitty,fish"), None);
}

#[test]
fn test_parse_monitors_json() {
    let on_json = br#"[{"name":"DP-1","dpmsStatus":true}]"#;
    assert_eq!(parse_monitors_json(on_json).unwrap(), Some(IdleState::Active));

    let off_json = br#"[{"name":"DP-1","dpmsStatus":false}]"#;
    assert_eq!(parse_monitors_json(off_json).unwrap(), Some(IdleState::Idle));

    let multi_one_on =
        br#"[{"name":"DP-1","dpmsStatus":false},{"name":"HDMI-1","dpmsStatus":true}]"#;
    assert_eq!(parse_monitors_json(multi_one_on).unwrap(), Some(IdleState::Active));

    let multi_both_off =
        br#"[{"name":"DP-1","dpmsStatus":false},{"name":"HDMI-1","dpmsStatus":false}]"#;
    assert_eq!(parse_monitors_json(multi_both_off).unwrap(), Some(IdleState::Idle));

    // Empty monitor responses represent transient reconfiguration, not confirmed idle
    assert_eq!(parse_monitors_json(b"[]").unwrap(), None);
    assert_eq!(parse_monitors_json(b"").unwrap(), None);
}

#[test]
fn test_resolve_socket_dir() {
    let custom = PathBuf::from("/custom/hypr/dir");
    let resolved = resolve_socket_dir(Some(&custom)).unwrap();
    assert_eq!(resolved, custom);

    let temp = tempdir().unwrap();
    let instance = "test_instance_1234";
    let hypr_dir = temp.path().join("hypr").join(instance);
    std::fs::create_dir_all(&hypr_dir).unwrap();
    std::fs::write(hypr_dir.join(".socket2.sock"), b"").unwrap();

    std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", instance);
    std::env::set_var("XDG_RUNTIME_DIR", temp.path().to_str().unwrap());

    let auto_resolved = resolve_socket_dir(None).unwrap();
    assert_eq!(auto_resolved, hypr_dir);
}

fn spawn_mock_socket1_responder(
    listener: UnixListener,
    active_window_resp: &'static [u8],
    monitors_resp: &'static [u8],
) {
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut req = [0u8; 64];
            let n = stream.read(&mut req).await.unwrap_or(0);
            let req_str = String::from_utf8_lossy(&req[..n]);

            if req_str.contains("j/activewindow") {
                let _ = stream.write_all(active_window_resp).await;
            } else if req_str.contains("j/monitors") {
                let _ = stream.write_all(monitors_resp).await;
            }
            let _ = stream.shutdown().await;
        }
    });
}

#[tokio::test]
async fn test_query_active_window_mock_socket1() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let listener = UnixListener::bind(&socket1_path).unwrap();

    spawn_mock_socket1_responder(
        listener,
        br#"{"class":"alacritty","title":"terminal session"}"#,
        b"[]",
    );

    let tracker = HyprlandWindowTracker::new().with_socket_dir(temp.path());
    let win = tracker.query_active_window().await.unwrap();
    assert_eq!(win, Some(WindowInfo::new("alacritty", "terminal session")));
}

#[tokio::test]
async fn test_query_monitors_mock_socket1() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let listener = UnixListener::bind(&socket1_path).unwrap();

    spawn_mock_socket1_responder(listener, b"{}", br#"[{"name":"DP-1","dpmsStatus":true}]"#);

    let idle = HyprlandIdleDetector::new().with_socket_dir(temp.path());
    let state = idle.query_monitors().await.unwrap();
    assert_eq!(state, Some(IdleState::Active));
}

#[tokio::test]
async fn test_window_tracker_streaming_events_and_title_commas() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let socket2_path = temp.path().join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    spawn_mock_socket1_responder(listener1, br#"{"class":"foot","title":"shell"}"#, b"[]");

    let listener2 = UnixListener::bind(&socket2_path).unwrap();
    let (event_tx, mut event_rx) = mpsc::channel(16);

    let mut tracker = HyprlandWindowTracker::new()
        .with_socket_dir(temp.path())
        .with_retry_interval(Duration::from_millis(50));

    tracker.start(event_tx).await.unwrap();

    let (mut client_stream, _) = listener2.accept().await.unwrap();

    let initial = event_rx.recv().await.unwrap();
    assert_eq!(initial, Some(WindowInfo::new("foot", "shell")));

    let events = [
        "activewindow>>firefox,Mozilla Firefox - PR #42, Ready\n",
        "activewindow>>code,src/main.rs - screentime, edited\n",
        "activewindow>>,\n",
        "activewindow>>kitty,term\n",
    ];

    for event in events {
        client_stream.write_all(event.as_bytes()).await.unwrap();
    }

    let ev1 = event_rx.recv().await.unwrap();
    assert_eq!(
        ev1,
        Some(WindowInfo::new(
            "firefox",
            "Mozilla Firefox - PR #42, Ready"
        ))
    );

    let ev2 = event_rx.recv().await.unwrap();
    assert_eq!(
        ev2,
        Some(WindowInfo::new("code", "src/main.rs - screentime, edited"))
    );

    let ev3 = event_rx.recv().await.unwrap();
    assert_eq!(ev3, None);

    let ev4 = event_rx.recv().await.unwrap();
    assert_eq!(ev4, Some(WindowInfo::new("kitty", "term")));

    tracker.stop().await.unwrap();
}

#[tokio::test]
async fn test_window_tracker_reconnection_resilience() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let socket2_path = temp.path().join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    spawn_mock_socket1_responder(listener1, b"{}", b"[]");

    let listener2 = UnixListener::bind(&socket2_path).unwrap();
    let (event_tx, mut event_rx) = mpsc::channel(16);

    let mut tracker = HyprlandWindowTracker::new()
        .with_socket_dir(temp.path())
        .with_retry_interval(Duration::from_millis(50));

    tracker.start(event_tx).await.unwrap();

    {
        let (mut stream1, _) = listener2.accept().await.unwrap();
        let _ = event_rx.recv().await.unwrap();

        stream1
            .write_all(b"activewindow>>alacritty,session 1\n")
            .await
            .unwrap();

        let ev1 = event_rx.recv().await.unwrap();
        assert_eq!(ev1, Some(WindowInfo::new("alacritty", "session 1")));
    }

    let (mut stream2, _) = listener2.accept().await.unwrap();
    let _ = event_rx.recv().await.unwrap();

    stream2
        .write_all(b"activewindow>>firefox,session 2\n")
        .await
        .unwrap();

    let ev2 = event_rx.recv().await.unwrap();
    assert_eq!(ev2, Some(WindowInfo::new("firefox", "session 2")));

    tracker.stop().await.unwrap();
}

#[tokio::test]
async fn test_idle_detector_dpms_streaming_and_polling() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let socket2_path = temp.path().join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    spawn_mock_socket1_responder(listener1, b"{}", br#"[{"name":"DP-1","dpmsStatus":true}]"#);

    let listener2 = UnixListener::bind(&socket2_path).unwrap();
    let (idle_tx, mut idle_rx) = mpsc::channel(16);

    let mut idle = HyprlandIdleDetector::new()
        .with_socket_dir(temp.path())
        .with_poll_interval(Duration::from_millis(100))
        .with_retry_interval(Duration::from_millis(50));

    idle.start(idle_tx).await.unwrap();

    let (mut client_stream, _) = listener2.accept().await.unwrap();

    let initial = idle_rx.recv().await.unwrap();
    assert_eq!(initial, IdleState::Active);
    assert!(!idle.is_idle());

    client_stream.write_all(b"dpms>>0\n").await.unwrap();

    let ev1 = idle_rx.recv().await.unwrap();
    assert_eq!(ev1, IdleState::Idle);
    assert!(idle.is_idle());

    client_stream.write_all(b"dpms>>1\n").await.unwrap();

    let ev2 = idle_rx.recv().await.unwrap();
    assert_eq!(ev2, IdleState::Active);
    assert!(!idle.is_idle());

    idle.stop().await.unwrap();
}

#[tokio::test]
async fn test_idle_detector_multi_monitor_dpms_per_output() {
    let temp = tempdir().unwrap();
    let socket1_path = temp.path().join(".socket.sock");
    let socket2_path = temp.path().join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    spawn_mock_socket1_responder(
        listener1,
        b"{}",
        br#"[{"name":"eDP-1","dpmsStatus":true},{"name":"HDMI-1","dpmsStatus":true}]"#,
    );

    let listener2 = UnixListener::bind(&socket2_path).unwrap();
    let (idle_tx, mut idle_rx) = mpsc::channel(16);

    let mut idle = HyprlandIdleDetector::new()
        .with_socket_dir(temp.path())
        .with_poll_interval(Duration::from_secs(60))
        .with_retry_interval(Duration::from_millis(50));

    idle.start(idle_tx).await.unwrap();

    let (mut client_stream, _) = listener2.accept().await.unwrap();
    assert_eq!(idle_rx.recv().await.unwrap(), IdleState::Active);

    // Turning off only one of two monitors must keep system Active
    client_stream.write_all(b"dpms>>eDP-1,0\n").await.unwrap();
    let unexpected = tokio::time::timeout(Duration::from_millis(120), idle_rx.recv()).await;
    assert!(unexpected.is_err(), "Should stay active while HDMI-1 is on");
    assert!(!idle.is_idle());

    // Turning off the second monitor transitions to Idle
    client_stream.write_all(b"dpms>>HDMI-1,0\n").await.unwrap();
    let ev = tokio::time::timeout(Duration::from_millis(500), idle_rx.recv())
        .await
        .expect("Expected Idle event when all monitors turn off")
        .unwrap();
    assert_eq!(ev, IdleState::Idle);
    assert!(idle.is_idle());

    idle.stop().await.unwrap();
}

#[tokio::test]
async fn test_hyprland_tracker_and_idle_with_session_manager() {
    let temp_db_dir = tempdir().unwrap();
    let db_path = temp_db_dir.path().join("test_screentime.db");
    let db_handle = DbActorHandle::spawn(&db_path).unwrap();

    let temp_sock_dir = tempdir().unwrap();
    let socket1_path = temp_sock_dir.path().join(".socket.sock");
    let socket2_path = temp_sock_dir.path().join(".socket2.sock");

    let listener1 = UnixListener::bind(&socket1_path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener1.accept().await {
            let mut req = [0u8; 32];
            let n = stream.read(&mut req).await.unwrap_or(0);
            let req_str = String::from_utf8_lossy(&req[..n]);

            if req_str.contains("j/activewindow") {
                let resp = br#"{"class":"alacritty","title":"terminal"}"#;
                let _ = stream.write_all(resp).await;
                let _ = stream.shutdown().await;
            } else if req_str.contains("j/monitors") {
                let resp = br#"[{"name":"DP-1","dpmsStatus":true}]"#;
                let _ = stream.write_all(resp).await;
                let _ = stream.shutdown().await;
            }
        }
    });

    let listener2 = UnixListener::bind(&socket2_path).unwrap();

    let tracker = HyprlandWindowTracker::new()
        .with_socket_dir(temp_sock_dir.path())
        .with_retry_interval(Duration::from_millis(50));

    let idle = HyprlandIdleDetector::new()
        .with_socket_dir(temp_sock_dir.path())
        .with_poll_interval(Duration::from_millis(100))
        .with_retry_interval(Duration::from_millis(50));

    let manager = SessionManager::new(db_handle.clone())
        .with_device_id("test-hyprland")
        .with_min_duration(0.0);
    let (url_tx, url_rx) = mpsc::channel(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let session_loop = tokio::spawn(async move {
        manager
            .run_loop(tracker, idle, url_rx, shutdown_rx)
            .await
            .unwrap();
    });

    let (mut sock2_tracker, _) = listener2.accept().await.unwrap();
    let (mut sock2_idle, _) = listener2.accept().await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    sock2_tracker
        .write_all(b"activewindow>>firefox,Mozilla Firefox - Rust\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    url_tx.send("https://crates.io".to_string()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    sock2_idle.write_all(b"dpms>>off\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    shutdown_tx.send(()).await.unwrap();
    session_loop.await.unwrap();

    let history = db_handle
        .get_sessions_in_range(0.0, f64::MAX)
        .await
        .unwrap();
    assert!(!history.is_empty(), "Should have recorded sessions in DB");
    assert!(
        history
            .iter()
            .any(|s| s.app_class == "alacritty" || s.app_class == "firefox"),
        "Sessions should include alacritty or firefox"
    );
}
