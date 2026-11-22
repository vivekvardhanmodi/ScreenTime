use screentime_core::{DbActorHandle, IdleDetector, IdleState, WindowInfo, WindowTracker};
use screentimed::wlroots::{
    compute_active_window, compute_idle_state, dispatch::OutgoingDrainer, parse_state_activated,
    ToplevelData, WaylandIdleDetector, WaylandIdleState, WlrForeignToplevelState,
    WlrForeignToplevelTracker,
};
use screentimed::SessionManager;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::{mpsc, Mutex};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::State;

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

struct EnvGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let original = std::env::var(key).ok();
        std::env::set_var(key, val);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(ref val) = self.original {
            std::env::set_var(self.key, val);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn test_parse_state_activated() {
    assert!(!parse_state_activated(&[]));

    let maximized = (State::Maximized as u32).to_ne_bytes();
    assert!(!parse_state_activated(&maximized));

    let minimized = (State::Minimized as u32).to_ne_bytes();
    assert!(!parse_state_activated(&minimized));

    let fullscreen = (State::Fullscreen as u32).to_ne_bytes();
    assert!(!parse_state_activated(&fullscreen));

    let activated = (State::Activated as u32).to_ne_bytes();
    assert!(parse_state_activated(&activated));

    let mut combo = Vec::new();
    combo.extend_from_slice(&maximized);
    combo.extend_from_slice(&activated);
    assert!(parse_state_activated(&combo));

    let mut combo_no_active = Vec::new();
    combo_no_active.extend_from_slice(&maximized);
    combo_no_active.extend_from_slice(&fullscreen);
    combo_no_active.extend_from_slice(&minimized);
    assert!(!parse_state_activated(&combo_no_active));

    assert!(!parse_state_activated(&[2, 0]));
    assert!(!parse_state_activated(&[2, 0, 0]));
}

#[test]
fn test_toplevel_latching() {
    let mut data = ToplevelData::default();

    assert_eq!(data.app_id, None);
    assert_eq!(data.title, None);
    assert!(!data.is_active);

    data.pending_app_id = Some("firefox".to_string());
    data.pending_title = Some("Google Search".to_string());
    data.pending_is_active = true;

    assert_eq!(data.app_id, None);
    assert_eq!(data.title, None);
    assert!(!data.is_active);

    data.commit();
    assert_eq!(data.app_id.as_deref(), Some("firefox"));
    assert_eq!(data.title.as_deref(), Some("Google Search"));
    assert!(data.is_active);
    assert_eq!(data.pending_app_id, None);
    assert_eq!(data.pending_title, None);

    data.pending_title = Some("YouTube - Video".to_string());
    data.pending_is_active = false;
    assert_eq!(data.title.as_deref(), Some("Google Search"));
    assert!(data.is_active);

    data.commit();
    assert_eq!(data.title.as_deref(), Some("YouTube - Video"));
    assert!(!data.is_active);
}

#[test]
fn test_toplevel_compute_active_window() {
    assert_eq!(compute_active_window(&[]), None);

    let win1 = ToplevelData {
        app_id: Some("firefox".to_string()),
        title: Some("Google".to_string()),
        is_active: false,
        ..Default::default()
    };
    assert_eq!(compute_active_window(std::slice::from_ref(&win1)), None);

    let win_empty_app = ToplevelData {
        app_id: Some("".to_string()),
        title: Some("Terminal".to_string()),
        is_active: true,
        ..Default::default()
    };
    assert_eq!(compute_active_window(&[win_empty_app]), None);

    let win_ws_app = ToplevelData {
        app_id: Some("   ".to_string()),
        title: Some("Terminal".to_string()),
        is_active: true,
        ..Default::default()
    };
    assert_eq!(compute_active_window(&[win_ws_app]), None);

    let win_active = ToplevelData {
        app_id: Some("org.gnome.Terminal".to_string()),
        title: Some("PR #42: fix, build, and test - bash".to_string()),
        is_active: true,
        ..Default::default()
    };
    let expected = Some(WindowInfo::new(
        "org.gnome.Terminal",
        "PR #42: fix, build, and test - bash",
    ));
    assert_eq!(compute_active_window(&[win1, win_active]), expected);
}

#[test]
fn test_wayland_idle_composite_calculation() {
    assert!(!compute_idle_state(vec![], 0, 0));
    assert!(!compute_idle_state(vec![true], 1, 0));
    assert!(compute_idle_state(vec![true], 1, 1));
    assert!(!compute_idle_state(vec![true], 1, 0));
    assert!(compute_idle_state(vec![false], 1, 0));
    assert!(!compute_idle_state(vec![false, true], 1, 0));
    assert!(compute_idle_state(vec![false, false], 1, 0));
    assert!(!compute_idle_state(vec![true, false], 1, 0));
    assert!(!compute_idle_state(vec![true], 2, 1));
    assert!(compute_idle_state(vec![true], 2, 2));
    assert!(!compute_idle_state(vec![true], 1, 0));
}

#[test]
fn test_wlr_foreign_toplevel_state_lifecycle() {
    let shared = Arc::new(RwLock::new(None));
    let mut state = WlrForeignToplevelState {
        registry: None,
        manager: None,
        windows: HashMap::new(),
        current_active: None,
        shared_active: shared.clone(),
        outgoing: VecDeque::new(),
    };

    assert_eq!(state.pop_outgoing(), None);

    let win = Some(WindowInfo::new("alacritty", "zsh"));
    state.outgoing.push_back(win.clone());
    assert_eq!(state.pop_outgoing(), Some(win));
    assert_eq!(state.pop_outgoing(), None);

    state.cleanup();
    assert!(state.windows.is_empty());
    assert!(state.manager.is_none());
}

#[test]
fn test_wayland_idle_state_lifecycle() {
    let shared = Arc::new(AtomicBool::new(false));
    let mut state = WaylandIdleState {
        registry: None,
        notifier: None,
        notification_by_global: HashMap::new(),
        idled_notifications: HashSet::new(),
        output_power_mgr: None,
        output_power_by_global: HashMap::new(),
        output_modes: HashMap::new(),
        seat_globals: HashMap::new(),
        output_globals: HashMap::new(),
        idle_timeout_ms: 60_000,
        current_is_idle: false,
        shared_is_idle: shared.clone(),
        outgoing: VecDeque::new(),
    };

    assert_eq!(state.pop_outgoing(), None);

    state.outgoing.push_back(IdleState::Idle);
    assert_eq!(state.pop_outgoing(), Some(IdleState::Idle));
    assert_eq!(state.pop_outgoing(), None);

    state.outgoing.push_back(IdleState::Active);
    assert_eq!(state.pop_outgoing(), Some(IdleState::Active));
    assert_eq!(state.pop_outgoing(), None);

    state.handle_global_remove(999);
    state.cleanup();
    assert!(state.notification_by_global.is_empty());
    assert!(state.output_power_by_global.is_empty());
    assert!(state.seat_globals.is_empty());
    assert!(state.output_globals.is_empty());
}

#[test]
fn test_wlroots_tracker_builder_and_defaults() {
    let tracker = WlrForeignToplevelTracker::new();
    let default_tracker = WlrForeignToplevelTracker::default();
    assert!(!tracker.is_running());
    assert!(!default_tracker.is_running());
}

#[test]
fn test_wayland_idle_detector_builder_and_defaults() {
    let detector = WaylandIdleDetector::new().with_idle_timeout(Duration::from_secs(300));
    assert_eq!(detector.idle_timeout(), Duration::from_secs(300));
    assert!(!detector.is_idle());
    assert!(!detector.is_running());

    let default_detector = WaylandIdleDetector::default();
    assert_eq!(default_detector.idle_timeout(), Duration::from_secs(180));
    assert!(!default_detector.is_idle());
    assert!(!default_detector.is_running());
}

#[tokio::test]
async fn test_wlroots_tracker_missing_display() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::set("WAYLAND_DISPLAY", "nonexistent-wayland-test-display-9999");
    let mut tracker = WlrForeignToplevelTracker::new();
    let (tx, _rx) = mpsc::channel(10);
    let res = tracker.start(tx).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_wayland_idle_missing_display() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::set("WAYLAND_DISPLAY", "nonexistent-wayland-test-display-9999");
    let mut detector = WaylandIdleDetector::new();
    let (tx, _rx) = mpsc::channel(10);
    let res = detector.start(tx).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_wlroots_tracker_query_active_window_disconnected() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::set("WAYLAND_DISPLAY", "nonexistent-wayland-test-display-9999");
    let tracker = WlrForeignToplevelTracker::new();
    let res = tracker.query_active_window().await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_wlroots_tracker_from_closed_socket_stream() {
    let (stream, peer) = std::os::unix::net::UnixStream::pair().expect("socket pair");
    drop(peer);
    let mut tracker = WlrForeignToplevelTracker::from_socket_stream(stream);
    let (tx, _rx) = mpsc::channel(10);
    let res = tracker.start(tx).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_wayland_idle_from_closed_socket_stream() {
    let (stream, peer) = std::os::unix::net::UnixStream::pair().expect("socket pair");
    drop(peer);
    let mut detector = WaylandIdleDetector::from_socket_stream(stream);
    let (tx, _rx) = mpsc::channel(10);
    let res = detector.start(tx).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_session_manager_with_wlroots_data() {
    let temp_dir = tempdir().expect("tempdir created");
    let db_path = temp_dir.path().join("test_wlroots_screentime.db");

    let db = DbActorHandle::spawn(&db_path).expect("db started");
    let mut session_mgr = SessionManager::new(db.clone());

    let win1_id = session_mgr
        .handle_window_change(Some(WindowInfo::new("kitty", "terminal - fish")), 100.0)
        .await
        .expect("window change handled")
        .expect("session created");

    let session1 = db.get_session(win1_id).await.unwrap().unwrap();
    assert_eq!(session1.app_class, "kitty");
    assert_eq!(session1.app_title.as_deref(), Some("terminal - fish"));

    let win2_id = session_mgr
        .handle_window_change(
            Some(WindowInfo::new("kitty", "cargo test - screentime")),
            105.0,
        )
        .await
        .expect("title update handled")
        .expect("session updated");

    let session2 = db.get_session(win2_id).await.unwrap().unwrap();
    assert_eq!(session2.app_class, "kitty");
    assert_eq!(
        session2.app_title.as_deref(),
        Some("cargo test - screentime")
    );

    session_mgr
        .handle_idle_change(IdleState::Idle, 110.0)
        .await
        .expect("idle state change handled");

    assert!(session_mgr.is_idle());

    session_mgr
        .handle_idle_change(IdleState::Active, 120.0)
        .await
        .expect("active state change handled");

    assert!(!session_mgr.is_idle());

    session_mgr
        .handle_window_change(None, 125.0)
        .await
        .expect("unfocused");

    assert_eq!(session_mgr.current_window(), None);
}
