use screentime_core::{DbActorHandle, IdleDetector, IdleState, UrlUpdate, WindowTracker};
use screentimed::x11::{
    clean_text_property, evaluate_idle_state, is_ignorable_x11_error, parse_wm_class_bytes,
    X11IdleDetector, X11WindowTracker,
};
use screentimed::SessionManager;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::x11_utils::X11Error;

#[test]
fn test_parse_wm_class_bytes_standard() {
    // ICCCM format: <instance>\0<class>\0
    let bytes = b"code\0Code\0";
    assert_eq!(parse_wm_class_bytes(bytes), Some("Code".to_string()));

    let firefox = b"Navigator\0firefox\0";
    assert_eq!(parse_wm_class_bytes(firefox), Some("firefox".to_string()));
}

#[test]
fn test_parse_wm_class_bytes_fallback_instance() {
    // When class is empty, instance is used
    let bytes = b"alacritty\0\0";
    assert_eq!(parse_wm_class_bytes(bytes), Some("alacritty".to_string()));

    let space_class = b"foot\0   \0";
    assert_eq!(parse_wm_class_bytes(space_class), Some("foot".to_string()));
}

#[test]
fn test_parse_wm_class_bytes_empty_and_null() {
    assert_eq!(parse_wm_class_bytes(b""), None);
    assert_eq!(parse_wm_class_bytes(b"\0"), None);
    assert_eq!(parse_wm_class_bytes(b"\0\0"), None);
    assert_eq!(parse_wm_class_bytes(b"   \0   \0"), None);
}

#[test]
fn test_parse_wm_class_bytes_no_null_terminator() {
    // Single string without trailing null
    assert_eq!(parse_wm_class_bytes(b"kitty"), Some("kitty".to_string()));
}

#[test]
fn test_clean_text_property() {
    assert_eq!(
        clean_text_property(b"screentime - bash\0"),
        Some("screentime - bash".to_string())
    );
    assert_eq!(
        clean_text_property(b"GitHub - \xE6\x97\xA5\xE6\x9C\xAC\xE8\xAA\x9E\0"),
        Some("GitHub - 日本語".to_string())
    );
    assert_eq!(clean_text_property(b""), None);
    assert_eq!(clean_text_property(b"\0"), None);
    assert_eq!(clean_text_property(b"   \0"), None);
}

#[test]
fn test_evaluate_idle_state() {
    let threshold = Duration::from_secs(120);

    // Below threshold, screensaver OFF -> Active
    assert_eq!(evaluate_idle_state(50_000, 0, threshold), IdleState::Active);

    // At threshold, screensaver OFF -> Idle
    assert_eq!(evaluate_idle_state(120_000, 0, threshold), IdleState::Idle);

    // Above threshold, screensaver OFF -> Idle
    assert_eq!(evaluate_idle_state(300_000, 0, threshold), IdleState::Idle);

    // Below threshold, but screensaver ON -> Idle
    assert_eq!(evaluate_idle_state(5_000, 1, threshold), IdleState::Idle);

    // 0ms since input, screensaver ON -> Idle
    assert_eq!(evaluate_idle_state(0, 1, threshold), IdleState::Idle);
}

#[test]
fn test_is_ignorable_x11_error() {
    // Transient X11 errors when window closes mid-query
    let bad_window = ReplyError::X11Error(X11Error {
        error_kind: ErrorKind::Window,
        error_code: 3,
        sequence: 1,
        bad_value: 0x1234,
        minor_opcode: 0,
        major_opcode: 20,
        extension_name: None,
        request_name: None,
    });
    assert!(is_ignorable_x11_error(&bad_window));

    let bad_drawable = ReplyError::X11Error(X11Error {
        error_kind: ErrorKind::Drawable,
        error_code: 9,
        sequence: 2,
        bad_value: 0x1234,
        minor_opcode: 0,
        major_opcode: 20,
        extension_name: None,
        request_name: None,
    });
    assert!(is_ignorable_x11_error(&bad_drawable));

    let bad_match = ReplyError::X11Error(X11Error {
        error_kind: ErrorKind::Match,
        error_code: 8,
        sequence: 3,
        bad_value: 0,
        minor_opcode: 0,
        major_opcode: 20,
        extension_name: None,
        request_name: None,
    });
    assert!(is_ignorable_x11_error(&bad_match));

    // Connection errors are NOT ignorable
    let conn_err = ReplyError::ConnectionError(x11rb::errors::ConnectionError::UnknownError);
    assert!(!is_ignorable_x11_error(&conn_err));
}

#[test]
fn test_x11_tracker_builder() {
    let tracker = X11WindowTracker::new().with_display(":42");
    assert_eq!(tracker.display(), Some(":42"));
    assert!(!tracker.is_running());
}

#[test]
fn test_x11_idle_detector_builder() {
    let idle = X11IdleDetector::new()
        .with_display(":42")
        .with_idle_threshold(Duration::from_secs(60))
        .with_poll_interval(Duration::from_millis(500));

    assert_eq!(idle.display(), Some(":42"));
    assert_eq!(idle.idle_threshold(), Duration::from_secs(60));
    assert_eq!(idle.poll_interval(), Duration::from_millis(500));
    assert!(!idle.is_running());
    assert!(!idle.is_idle());
}

#[tokio::test]
async fn test_x11_invalid_display_graceful_failure() {
    let mut tracker = X11WindowTracker::new().with_display(":99999");
    let (tx, _rx) = mpsc::channel(16);
    let res = tracker.start(tx).await;
    assert!(res.is_err(), "Expected connection error on invalid display");

    let mut idle = X11IdleDetector::new().with_display(":99999");
    let (idle_tx, _idle_rx) = mpsc::channel(16);
    let idle_res = idle.start(idle_tx).await;
    assert!(
        idle_res.is_err(),
        "Expected connection error on invalid display"
    );
}

#[tokio::test]
async fn test_x11_window_tracker_live_or_skip() {
    let Ok((_conn, _)) = x11rb::rust_connection::RustConnection::connect(None) else {
        eprintln!("Skipping live X11 test: no X server reachable on DISPLAY");
        return;
    };

    let mut tracker = X11WindowTracker::new();
    let (tx, mut rx) = mpsc::channel(16);

    tracker.start(tx).await.expect("tracker started");
    assert!(tracker.is_running());

    // Should receive initial active window (or None)
    let initial_msg = rx.recv().await;
    assert!(initial_msg.is_some());

    // query_active_window should return matching state
    let queried = tracker
        .query_active_window()
        .await
        .expect("query active window");
    assert_eq!(initial_msg.unwrap(), queried);

    tracker.stop().await.expect("tracker stopped");
    assert!(!tracker.is_running());
}

#[tokio::test]
async fn test_x11_idle_detector_live_or_skip() {
    let Ok((_conn, _)) = x11rb::rust_connection::RustConnection::connect(None) else {
        eprintln!("Skipping live X11 idle test: no X server reachable on DISPLAY");
        return;
    };

    let mut idle = X11IdleDetector::new()
        .with_idle_threshold(Duration::from_secs(120))
        .with_poll_interval(Duration::from_millis(100));

    let (tx, mut rx) = mpsc::channel(16);

    idle.start(tx).await.expect("idle detector started");
    assert!(idle.is_running());

    // Should receive initial idle state
    let initial = rx.recv().await;
    assert!(initial.is_some());

    // Check one-off queries
    let duration = idle.query_idle_duration().await;
    assert!(duration.is_ok());

    let state = idle.query_idle_state().await;
    assert!(state.is_ok());

    idle.stop().await.expect("idle detector stopped");
    assert!(!idle.is_running());
}

#[tokio::test]
async fn test_x11_with_session_manager_integration() {
    let Ok((_conn, _)) = x11rb::rust_connection::RustConnection::connect(None) else {
        eprintln!("Skipping live X11 SessionManager test: no X server reachable on DISPLAY");
        return;
    };

    let temp_dir = tempdir().expect("tempdir");
    let db_path = temp_dir.path().join("test_screentime.db");

    let db = DbActorHandle::spawn(&db_path).expect("db handle");
    let sm = SessionManager::new(db.clone());

    let tracker = X11WindowTracker::new();
    let idle = X11IdleDetector::new().with_poll_interval(Duration::from_millis(50));

    let (_url_tx, url_rx) = mpsc::channel::<UrlUpdate>(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let runner = tokio::spawn(async move { sm.run_loop(tracker, idle, url_rx, shutdown_rx).await });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = shutdown_tx.send(()).await;
    let res = runner.await.expect("runner task completed");
    assert!(res.is_ok(), "SessionManager run_loop exited successfully");
}
