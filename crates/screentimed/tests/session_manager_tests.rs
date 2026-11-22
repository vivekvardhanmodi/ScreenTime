use async_trait::async_trait;
use screentime_core::{
    DbActorHandle, IdleDetector, IdleState, TitleRulesCache, TrackerError, UrlUpdate, WindowInfo,
    WindowTracker,
};
use screentimed::{is_browser_app, SessionManager};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::{mpsc, Mutex};

#[derive(Clone)]
struct MockWindowTracker {
    active_window: Arc<Mutex<Option<WindowInfo>>>,
    tx: Arc<Mutex<Option<mpsc::Sender<Option<WindowInfo>>>>>,
}

impl MockWindowTracker {
    fn new(initial: Option<WindowInfo>) -> Self {
        Self {
            active_window: Arc::new(Mutex::new(initial)),
            tx: Arc::new(Mutex::new(None)),
        }
    }

    async fn set_window(&self, window: Option<WindowInfo>) {
        *self.active_window.lock().await = window.clone();
        if let Some(ref tx) = *self.tx.lock().await {
            let _ = tx.send(window).await;
        }
    }

    async fn set_window_silently(&self, window: Option<WindowInfo>) {
        *self.active_window.lock().await = window;
    }
}

#[async_trait]
impl WindowTracker for MockWindowTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        *self.tx.lock().await = Some(tx);
        Ok(())
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        Ok(self.active_window.lock().await.clone())
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        *self.tx.lock().await = None;
        Ok(())
    }
}

#[derive(Clone)]
struct MockIdleDetector {
    is_idle: Arc<AtomicBool>,
    tx: Arc<Mutex<Option<mpsc::Sender<IdleState>>>>,
}

impl MockIdleDetector {
    fn new(initial_idle: bool) -> Self {
        Self {
            is_idle: Arc::new(AtomicBool::new(initial_idle)),
            tx: Arc::new(Mutex::new(None)),
        }
    }

    async fn set_idle_state(&self, state: IdleState) {
        self.is_idle.store(state.is_idle(), Ordering::SeqCst);
        if let Some(ref tx) = *self.tx.lock().await {
            let _ = tx.send(state).await;
        }
    }
}

#[async_trait]
impl IdleDetector for MockIdleDetector {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        *self.tx.lock().await = Some(tx);
        Ok(())
    }

    fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        *self.tx.lock().await = None;
        Ok(())
    }
}

#[test]
fn test_browser_app_detection_avoids_substring_false_positives() {
    assert!(is_browser_app("google-chrome"));
    assert!(is_browser_app("chrome.exe"));
    assert!(is_browser_app(
        "chrome-agimnkijcaahngcdmfeangaknmldooml-Default"
    ));
    assert!(is_browser_app("org.chromium.Chromium"));
    assert!(is_browser_app("firefox"));
    assert!(is_browser_app("org.mozilla.firefox"));
    assert!(is_browser_app("zen-browser"));
    assert!(is_browser_app("app.zen_browser.zen"));
    assert!(is_browser_app("microsoft-edge-stable"));
    assert!(is_browser_app("msedge.exe"));
    assert!(is_browser_app("opera.exe"));
    assert!(is_browser_app("brave-browser"));

    assert!(!is_browser_app("zenity"));
    assert!(!is_browser_app("ledger"));
    assert!(!is_browser_app("knowledge-base"));
    assert!(!is_browser_app("cooperative"));
    assert!(!is_browser_app("alacritty"));
}

#[tokio::test]
async fn test_window_focus_and_alt_tab_noise_filtering() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_noise.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let win_a = WindowInfo::new("firefox", "Mozilla Firefox");
    let win_b = WindowInfo::new("alacritty", "Terminal");
    let win_c = WindowInfo::new("code", "VS Code");

    let id_a = sm
        .handle_window_change(Some(win_a.clone()), 100.0)
        .await
        .unwrap()
        .expect("Session A created");

    let id_a2 = sm
        .handle_window_change(Some(win_a.clone()), 100.2)
        .await
        .unwrap()
        .expect("Session A preserved");
    assert_eq!(id_a, id_a2);

    let id_b = sm
        .handle_window_change(Some(win_b.clone()), 100.5)
        .await
        .unwrap()
        .expect("Session B created");
    assert_ne!(id_a, id_b);

    assert!(db.get_session(id_a).await.unwrap().is_none());

    let id_c = sm
        .handle_window_change(Some(win_c.clone()), 103.5)
        .await
        .unwrap()
        .expect("Session C created");

    let session_b = db
        .get_session(id_b)
        .await
        .unwrap()
        .expect("Session B must exist");
    assert_eq!(session_b.app_class, "alacritty");
    assert_eq!(session_b.start_time, 100.5);
    assert_eq!(session_b.end_time, 103.5);
    assert_eq!(session_b.duration(), 3.0);

    sm.handle_shutdown(106.0).await.unwrap();
    let session_c = db
        .get_session(id_c)
        .await
        .unwrap()
        .expect("Session C must exist");
    assert_eq!(session_c.end_time, 106.0);
}

#[tokio::test]
async fn test_idle_state_transitions() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_idle.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let win = WindowInfo::new("editor", "main.rs");

    let id1 = sm
        .handle_window_change(Some(win.clone()), 200.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_idle_change(IdleState::Idle, 205.0).await.unwrap();
    assert!(sm.is_idle());
    assert!(sm.current_session_id().is_none());

    let session1 = db.get_session(id1).await.unwrap().unwrap();
    assert_eq!(session1.end_time, 205.0);

    let win2 = WindowInfo::new("browser", "Search");
    sm.handle_window_change(Some(win2.clone()), 210.0)
        .await
        .unwrap();
    assert!(sm.current_session_id().is_none());
    assert_eq!(sm.current_window(), Some(&win2));

    sm.handle_idle_change(IdleState::Active, 220.0)
        .await
        .unwrap();
    assert!(!sm.is_idle());
    let id2 = sm
        .current_session_id()
        .expect("New session started on resume");

    sm.handle_shutdown(225.0).await.unwrap();
    let session2 = db.get_session(id2).await.unwrap().unwrap();
    assert_eq!(session2.app_class, "browser");
    assert_eq!(session2.start_time, 220.0);
    assert_eq!(session2.end_time, 225.0);
}

#[tokio::test]
async fn test_heartbeat_after_long_idle_preserves_resumed_session() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_idle_hb.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());
    let browser = WindowInfo::new("google-chrome", "Google Chrome");

    let id1 = sm
        .handle_window_change(Some(browser), 100.0)
        .await
        .unwrap()
        .unwrap();
    sm.handle_browser_url("https://rust-lang.org".to_string(), 100.5)
        .await
        .unwrap();
    sm.handle_heartbeat(105.0).await.unwrap();

    sm.handle_idle_change(IdleState::Idle, 110.0).await.unwrap();
    sm.handle_heartbeat(200.0).await.unwrap();

    sm.handle_idle_change(IdleState::Active, 300.0)
        .await
        .unwrap();
    let id2 = sm
        .current_session_id()
        .expect("Resumed session must exist");
    assert_ne!(id1, id2);

    sm.handle_heartbeat(310.0).await.unwrap();
    assert_eq!(sm.current_session_id(), Some(id2));

    let resumed = db
        .get_session(id2)
        .await
        .unwrap()
        .expect("Resumed session must not be deleted by first post-idle heartbeat");
    assert_eq!(resumed.start_time, 300.0);
    assert_eq!(resumed.end_time, 310.0);
    assert_eq!(resumed.website_url.as_deref(), Some("https://rust-lang.org"));
}

#[tokio::test]
async fn test_browser_url_delayed_update_and_tab_switch() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_browser_url.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let browser = WindowInfo::new("google-chrome", "Google Chrome");

    let session_id = sm
        .handle_window_change(Some(browser.clone()), 300.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_browser_url("https://crates.io".to_string(), 301.2)
        .await
        .unwrap();

    assert_eq!(sm.current_session_id(), Some(session_id));
    assert_eq!(sm.current_url(), Some("https://crates.io"));

    let row1 = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(row1.website_url.as_deref(), Some("https://crates.io"));

    sm.handle_browser_url("https://github.com".to_string(), 308.0)
        .await
        .unwrap();

    let new_session_id = sm.current_session_id().unwrap();
    assert_ne!(new_session_id, session_id);
    assert_eq!(sm.current_url(), Some("https://github.com"));

    let closed_row1 = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(closed_row1.start_time, 300.0);
    assert_eq!(closed_row1.end_time, 308.0);
    assert_eq!(
        closed_row1.website_url.as_deref(),
        Some("https://crates.io")
    );

    let row2 = db.get_session(new_session_id).await.unwrap().unwrap();
    assert_eq!(row2.start_time, 308.0);
    assert_eq!(row2.website_url.as_deref(), Some("https://github.com"));
}

#[tokio::test]
async fn test_multi_browser_url_isolation() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_multi_browser.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let chrome_id = sm
        .handle_window_change(
            Some(WindowInfo::new("google-chrome", "Google Chrome")),
            100.0,
        )
        .await
        .unwrap()
        .unwrap();

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://github.com".to_string()),
            Some("GitHub".to_string()),
        ),
        100.5,
    )
    .await
    .unwrap();

    // Background Firefox event must not overwrite or split active Chrome session
    sm.handle_url_update(
        UrlUpdate::new(
            "firefox",
            Some("https://mozilla.org".to_string()),
            Some("Mozilla".to_string()),
        ),
        103.0,
    )
    .await
    .unwrap();

    assert_eq!(sm.current_session_id(), Some(chrome_id));
    assert_eq!(sm.current_url(), Some("https://github.com"));

    let chrome_row = db.get_session(chrome_id).await.unwrap().unwrap();
    assert_eq!(chrome_row.website_url.as_deref(), Some("https://github.com"));

    // Focusing Firefox should inherit Firefox's buffered URL
    let firefox_id = sm
        .handle_window_change(Some(WindowInfo::new("firefox", "Mozilla Firefox")), 105.0)
        .await
        .unwrap()
        .unwrap();

    let firefox_row = db.get_session(firefox_id).await.unwrap().unwrap();
    assert_eq!(firefox_row.website_url.as_deref(), Some("https://mozilla.org"));
}

#[tokio::test]
async fn test_pwa_url_update_writes_and_updates_tab_title() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_pwa_title.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let pwa_window = WindowInfo::new("chrome-agimnkijcaahngcdmfeangaknmldooml-Default", "YouTube");
    let session_id = sm
        .handle_window_change(Some(pwa_window), 500.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=abc123".to_string()),
            Some("Rust Async Deep Dive - YouTube".to_string()),
        ),
        500.6,
    )
    .await
    .unwrap();

    let row = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(row.app_title.as_deref(), Some("Rust Async Deep Dive - YouTube"));
    assert_eq!(row.website_url.as_deref(), Some("https://www.youtube.com/watch?v=abc123"));

    // Same URL with updated SPA/PWA document title must update the title in-place
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=abc123".to_string()),
            Some("(2) Rust Async Deep Dive - YouTube".to_string()),
        ),
        505.0,
    )
    .await
    .unwrap();

    assert_eq!(sm.current_session_id(), Some(session_id));
    let updated_row = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(
        updated_row.app_title.as_deref(),
        Some("(2) Rust Async Deep Dive - YouTube")
    );
}

#[tokio::test]
async fn test_heartbeats_and_sleep_suspend_detection() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_heartbeat.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    let win = WindowInfo::new("kitty", "terminal");

    let session_id = sm
        .handle_window_change(Some(win.clone()), 400.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_heartbeat(405.0).await.unwrap();
    let row = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(row.end_time, 405.0);

    sm.handle_heartbeat(410.0).await.unwrap();
    let row = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(row.end_time, 410.0);

    sm.handle_heartbeat(530.0).await.unwrap();

    let closed_session = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(closed_session.end_time, 410.0);

    let new_session_id = sm.current_session_id().unwrap();
    assert_ne!(new_session_id, session_id);

    let wake_session = db.get_session(new_session_id).await.unwrap().unwrap();
    assert_eq!(wake_session.start_time, 530.0);
}

#[tokio::test]
async fn test_title_rule_resolution() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_rules.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut rules_map = HashMap::new();
    rules_map.insert("kitty".to_string(), vec!["nvim".to_string()]);
    let title_rules = TitleRulesCache::new(rules_map);

    let mut sm = SessionManager::new(db.clone()).with_title_rules(title_rules);

    let nvim_window = WindowInfo::new("kitty", "nvim src/main.rs");
    let session_id = sm
        .handle_window_change(Some(nvim_window), 600.0)
        .await
        .unwrap()
        .unwrap();

    let session = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(session.app_class, "nvim");

    let bash_window = WindowInfo::new("kitty", "bash");
    let bash_session_id = sm
        .handle_window_change(Some(bash_window), 605.0)
        .await
        .unwrap()
        .unwrap();

    let bash_session = db.get_session(bash_session_id).await.unwrap().unwrap();
    assert_eq!(bash_session.app_class, "kitty");
}

#[tokio::test]
async fn test_event_loop_run_with_mocks() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_loop.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let initial_win = WindowInfo::new("firefox", "Mozilla Firefox");
    let tracker = MockWindowTracker::new(Some(initial_win.clone()));
    let tracker_clone = tracker.clone();

    let idle_detector = MockIdleDetector::new(false);
    let idle_clone = idle_detector.clone();

    let (url_tx, url_rx) = mpsc::channel(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let sm = SessionManager::new(db.clone());

    let runner = tokio::spawn(async move {
        sm.run_loop(tracker, idle_detector, url_rx, shutdown_rx)
            .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    url_tx
        .send("https://rust-lang.org".to_string())
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    tracker_clone
        .set_window(Some(WindowInfo::new("alacritty", "Terminal")))
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    idle_clone.set_idle_state(IdleState::Idle).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    shutdown_tx.send(()).await.unwrap();

    let res = runner.await.expect("Runner panicked");
    assert!(res.is_ok());

    let all_sessions = db
        .get_sessions_in_range(0.0, 2_000_000_000.0)
        .await
        .unwrap();
    assert_eq!(all_sessions.len(), 2);
    assert_eq!(all_sessions[0].app_class, "firefox");
    assert_eq!(
        all_sessions[0].website_url.as_deref(),
        Some("https://rust-lang.org")
    );
    assert_eq!(all_sessions[1].app_class, "alacritty");
}

#[tokio::test]
async fn test_run_loop_requeries_active_window_on_idle_resume() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_requery.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let tracker = MockWindowTracker::new(Some(WindowInfo::new("kitty", "initial")));
    let tracker_clone = tracker.clone();

    let idle_detector = MockIdleDetector::new(false);
    let idle_clone = idle_detector.clone();

    let (_url_tx, url_rx) = mpsc::channel::<String>(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let sm = SessionManager::new(db.clone()).with_min_duration(0.0);
    let runner = tokio::spawn(async move {
        sm.run_loop(tracker, idle_detector, url_rx, shutdown_rx)
            .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    idle_clone.set_idle_state(IdleState::Idle).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    tracker_clone
        .set_window_silently(Some(WindowInfo::new("code", "after-resume")))
        .await;

    idle_clone.set_idle_state(IdleState::Active).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    shutdown_tx.send(()).await.unwrap();
    runner.await.expect("Runner panicked").unwrap();

    let sessions = db
        .get_sessions_in_range(0.0, 2_000_000_000.0)
        .await
        .unwrap();
    assert!(
        sessions.iter().any(|s| s.app_class == "code"),
        "Expected re-queried window 'code' after resume, got: {sessions:?}"
    );
}
