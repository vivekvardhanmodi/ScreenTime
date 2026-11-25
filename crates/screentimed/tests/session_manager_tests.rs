use async_trait::async_trait;
use screentime_core::{
    DbActorHandle, IdleDetector, IdleState, TitleRulesCache, TrackerError, UrlUpdate, WindowInfo,
    WindowTracker,
};
use screentimed::{is_browser_app, SessionManager, SleepEvent};
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
    let id2 = sm.current_session_id().expect("Resumed session must exist");
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
    assert_eq!(
        resumed.website_url.as_deref(),
        Some("https://rust-lang.org")
    );
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
    assert_eq!(
        chrome_row.website_url.as_deref(),
        Some("https://github.com")
    );

    // Focusing Firefox should inherit Firefox's buffered URL
    let firefox_id = sm
        .handle_window_change(Some(WindowInfo::new("firefox", "Mozilla Firefox")), 105.0)
        .await
        .unwrap()
        .unwrap();

    let firefox_row = db.get_session(firefox_id).await.unwrap().unwrap();
    assert_eq!(
        firefox_row.website_url.as_deref(),
        Some("https://mozilla.org")
    );
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
    assert_eq!(
        row.app_title.as_deref(),
        Some("Rust Async Deep Dive - YouTube")
    );
    assert_eq!(
        row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=abc123")
    );

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

#[tokio::test]
async fn test_pwa_preserves_extension_title_across_idle_sleep_lid_and_heartbeat_resume() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_pwa_resume_titles.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());
    let pwa_window = WindowInfo::new("chrome-agimnkijcaahngcdmfeangaknmldooml-Default", "YouTube");

    let initial_id = sm
        .handle_window_change(Some(pwa_window), 100.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=abc123".to_string()),
            Some("Rust Async Deep Dive - YouTube".to_string()),
        ),
        100.5,
    )
    .await
    .unwrap();
    sm.handle_heartbeat(105.0).await.unwrap();

    sm.handle_idle_change(IdleState::Idle, 110.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 120.0)
        .await
        .unwrap();
    let idle_resumed_id = sm.current_session_id().expect("Idle resume session");
    assert_ne!(idle_resumed_id, initial_id);
    let idle_row = db.get_session(idle_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        idle_row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=abc123")
    );
    assert_eq!(
        idle_row.app_title.as_deref(),
        Some("Rust Async Deep Dive - YouTube"),
        "idle resume must preserve extension title instead of Hyprland generic title"
    );

    sm.handle_heartbeat(125.0).await.unwrap();
    sm.handle_sleep_event(SleepEvent::Sleep, 130.0)
        .await
        .unwrap();
    sm.handle_sleep_event(SleepEvent::Wake, 140.0)
        .await
        .unwrap();
    let sleep_resumed_id = sm.current_session_id().expect("Sleep resume session");
    assert_ne!(sleep_resumed_id, idle_resumed_id);
    let sleep_row = db.get_session(sleep_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        sleep_row.app_title.as_deref(),
        Some("Rust Async Deep Dive - YouTube"),
        "sleep wake must preserve extension title"
    );

    sm.handle_heartbeat(145.0).await.unwrap();
    sm.handle_sleep_event(
        SleepEvent::LidClosed {
            has_external_display: false,
        },
        150.0,
    )
    .await
    .unwrap();
    sm.handle_sleep_event(SleepEvent::LidOpened, 160.0)
        .await
        .unwrap();
    let lid_resumed_id = sm.current_session_id().expect("Lid open resume session");
    assert_ne!(lid_resumed_id, sleep_resumed_id);
    let lid_row = db.get_session(lid_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        lid_row.app_title.as_deref(),
        Some("Rust Async Deep Dive - YouTube"),
        "lid open must preserve extension title"
    );

    sm.handle_heartbeat(165.0).await.unwrap();
    sm.handle_heartbeat(265.0).await.unwrap();
    let hb_resumed_id = sm.current_session_id().expect("Heartbeat jump session");
    assert_ne!(hb_resumed_id, lid_resumed_id);
    let hb_row = db.get_session(hb_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        hb_row.app_title.as_deref(),
        Some("Rust Async Deep Dive - YouTube"),
        "heartbeat jump resume must preserve extension title"
    );
}

#[tokio::test]
async fn test_buffered_url_update_while_non_browser_focused_preserves_title() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_buffered_title.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    sm.handle_window_change(Some(WindowInfo::new("kitty", "nvim")), 100.0)
        .await
        .unwrap();

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://gemini.google.com/app/123".to_string()),
            Some("Gemini - Rust Architecture Chat".to_string()),
        ),
        102.0,
    )
    .await
    .unwrap();

    let pwa_id = sm
        .handle_window_change(
            Some(WindowInfo::new("chrome-gemini-Default", "Gemini")),
            105.0,
        )
        .await
        .unwrap()
        .unwrap();

    let pwa_row = db.get_session(pwa_id).await.unwrap().unwrap();
    assert_eq!(
        pwa_row.website_url.as_deref(),
        Some("https://gemini.google.com/app/123")
    );
    assert_eq!(
        pwa_row.app_title.as_deref(),
        Some("Gemini - Rust Architecture Chat"),
        "focusing PWA after buffered UrlUpdate must restore buffered title"
    );
}

#[tokio::test]
async fn test_chrome_and_pwa_cached_state_isolated_per_app_class() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_pwa_isolation.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());
    let chrome_win = WindowInfo::new("google-chrome", "Google Chrome");
    let pwa_win = WindowInfo::new("chrome-agimnkijcaahngcdmfeangaknmldooml-Default", "YouTube");

    sm.handle_window_change(Some(chrome_win.clone()), 100.0)
        .await
        .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://github.com/rust-lang/rust".to_string()),
            Some("rust-lang/rust".to_string()),
        ),
        100.5,
    )
    .await
    .unwrap();

    sm.handle_idle_change(IdleState::Idle, 102.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 103.0)
        .await
        .unwrap();
    let chrome_resumed_id = sm.current_session_id().expect("Resumed Chrome session");
    let chrome_resumed_row = db.get_session(chrome_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        chrome_resumed_row.website_url.as_deref(),
        Some("https://github.com/rust-lang/rust")
    );
    assert_eq!(
        chrome_resumed_row.app_title.as_deref(),
        Some("rust-lang/rust")
    );

    let pwa_id1 = sm
        .handle_window_change(Some(pwa_win), 105.0)
        .await
        .unwrap()
        .unwrap();
    let pwa_row1 = db.get_session(pwa_id1).await.unwrap().unwrap();
    assert_eq!(pwa_row1.website_url, None);
    assert_eq!(pwa_row1.app_title.as_deref(), Some("YouTube"));

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=xyz".to_string()),
            Some("Systems Programming Talk - YouTube".to_string()),
        ),
        105.5,
    )
    .await
    .unwrap();
    let pwa_updated = db.get_session(pwa_id1).await.unwrap().unwrap();
    assert_eq!(
        pwa_updated.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=xyz")
    );
    assert_eq!(
        pwa_updated.app_title.as_deref(),
        Some("Systems Programming Talk - YouTube")
    );

    sm.handle_idle_change(IdleState::Idle, 108.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 110.0)
        .await
        .unwrap();
    let pwa_resumed_id = sm.current_session_id().expect("Resumed PWA session");
    let pwa_resumed_row = db.get_session(pwa_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        pwa_resumed_row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=xyz")
    );
    assert_eq!(
        pwa_resumed_row.app_title.as_deref(),
        Some("Systems Programming Talk - YouTube")
    );

    let chrome_id2 = sm
        .handle_window_change(Some(chrome_win), 115.0)
        .await
        .unwrap()
        .unwrap();
    let chrome_row2 = db.get_session(chrome_id2).await.unwrap().unwrap();
    assert_eq!(chrome_row2.website_url, None);
    assert_eq!(chrome_row2.app_title.as_deref(), Some("Google Chrome"));
}

#[tokio::test]
async fn test_none_url_update_preserves_other_pwa_cached_state() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_none_preserves_pwa.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());
    let pwa_win = WindowInfo::new("chrome-agimnkijcaahngcdmfeangaknmldooml-Default", "YouTube");

    sm.handle_window_change(Some(pwa_win.clone()), 100.0)
        .await
        .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=xyz".to_string()),
            Some("Systems Programming Talk - YouTube".to_string()),
        ),
        100.5,
    )
    .await
    .unwrap();

    sm.handle_idle_change(IdleState::Idle, 102.0).await.unwrap();
    sm.handle_url_update(UrlUpdate::new("chrome", None, None), 103.0)
        .await
        .unwrap();
    sm.handle_idle_change(IdleState::Active, 104.0)
        .await
        .unwrap();
    let pwa_resumed_id = sm.current_session_id().expect("Resumed PWA session");
    let pwa_resumed_row = db.get_session(pwa_resumed_id).await.unwrap().unwrap();
    assert_eq!(
        pwa_resumed_row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=xyz"),
        "WINDOW_ID_NONE while idle must not wipe YouTube PWA cached URL"
    );
    assert_eq!(
        pwa_resumed_row.app_title.as_deref(),
        Some("Systems Programming Talk - YouTube"),
        "WINDOW_ID_NONE while idle must preserve YouTube PWA cached title"
    );

    sm.handle_window_change(
        Some(WindowInfo::new("google-chrome", "Google Chrome")),
        105.0,
    )
    .await
    .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://github.com/rust-lang/rust".to_string()),
            Some("rust-lang/rust".to_string()),
        ),
        105.5,
    )
    .await
    .unwrap();

    sm.handle_url_update(UrlUpdate::new("chrome", None, None), 108.0)
        .await
        .unwrap();
    let cleared_chrome_id = sm.current_session_id().expect("New Tab session");
    let cleared_chrome_row = db.get_session(cleared_chrome_id).await.unwrap().unwrap();
    assert_eq!(cleared_chrome_row.website_url, None);
    assert_eq!(
        cleared_chrome_row.app_title.as_deref(),
        Some("Google Chrome")
    );

    sm.handle_idle_change(IdleState::Idle, 109.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 110.0)
        .await
        .unwrap();
    let chrome_resumed_id = sm.current_session_id().expect("Resumed New Tab session");
    let chrome_resumed_row = db.get_session(chrome_resumed_id).await.unwrap().unwrap();
    assert_eq!(chrome_resumed_row.website_url, None);

    sm.handle_window_change(Some(WindowInfo::new("kitty", "nvim")), 115.0)
        .await
        .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://example.com/stale".to_string()),
            Some("Stale Pending".to_string()),
        ),
        116.0,
    )
    .await
    .unwrap();
    sm.handle_url_update(UrlUpdate::new("chrome", None, None), 117.0)
        .await
        .unwrap();

    let pwa_after_focus_loss_id = sm
        .handle_window_change(Some(pwa_win), 120.0)
        .await
        .unwrap()
        .unwrap();
    let pwa_after_focus_loss_row = db
        .get_session(pwa_after_focus_loss_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pwa_after_focus_loss_row.website_url, None);
    assert_eq!(
        pwa_after_focus_loss_row.app_title.as_deref(),
        Some("YouTube")
    );
}

#[tokio::test]
async fn test_buffered_pending_url_consumed_on_window_switch_survives_resume() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_pending_switch_resume.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());
    let pwa_win = WindowInfo::new("chrome-agimnkijcaahngcdmfeangaknmldooml-Default", "YouTube");

    sm.handle_window_change(Some(WindowInfo::new("kitty", "nvim")), 100.0)
        .await
        .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://www.youtube.com/watch?v=pending123".to_string()),
            Some("Buffered Video - YouTube".to_string()),
        ),
        102.0,
    )
    .await
    .unwrap();

    let switched_id = sm
        .handle_window_change(Some(pwa_win), 105.0)
        .await
        .unwrap()
        .unwrap();
    let switched_row = db.get_session(switched_id).await.unwrap().unwrap();
    assert_eq!(
        switched_row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=pending123")
    );
    assert_eq!(
        switched_row.app_title.as_deref(),
        Some("Buffered Video - YouTube")
    );

    sm.handle_idle_change(IdleState::Idle, 108.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 112.0)
        .await
        .unwrap();
    let resumed_id = sm.current_session_id().expect("Resumed PWA session");
    assert_ne!(resumed_id, switched_id);
    let resumed_row = db.get_session(resumed_id).await.unwrap().unwrap();
    assert_eq!(
        resumed_row.website_url.as_deref(),
        Some("https://www.youtube.com/watch?v=pending123")
    );
    assert_eq!(
        resumed_row.app_title.as_deref(),
        Some("Buffered Video - YouTube")
    );
}

#[tokio::test]
async fn test_incognito_after_normal_chrome_and_non_browser_stays_clean_across_resume() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_incognito_resume_clean.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    sm.handle_window_change(
        Some(WindowInfo::new("google-chrome", "GitHub - Google Chrome")),
        100.0,
    )
    .await
    .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://github.com/rust-lang/rust".to_string()),
            Some("GitHub".to_string()),
        ),
        100.4,
    )
    .await
    .unwrap();

    sm.handle_window_change(Some(WindowInfo::new("kitty", "zsh")), 105.0)
        .await
        .unwrap();

    let incognito_id = sm
        .handle_window_change(
            Some(WindowInfo::new(
                "google-chrome",
                "New Incognito Tab - Google Chrome",
            )),
            110.0,
        )
        .await
        .unwrap()
        .unwrap();
    let incognito_row = db.get_session(incognito_id).await.unwrap().unwrap();
    assert_eq!(incognito_row.website_url, None);
    assert_eq!(
        incognito_row.app_title.as_deref(),
        Some("New Incognito Tab - Google Chrome")
    );

    sm.handle_idle_change(IdleState::Idle, 115.0).await.unwrap();
    sm.handle_idle_change(IdleState::Active, 120.0)
        .await
        .unwrap();
    let resumed_id = sm.current_session_id().expect("Resumed Incognito session");
    let resumed_row = db.get_session(resumed_id).await.unwrap().unwrap();
    assert_eq!(resumed_row.website_url, None);
    assert_eq!(
        resumed_row.app_title.as_deref(),
        Some("New Incognito Tab - Google Chrome")
    );
}

#[tokio::test]
async fn test_incognito_after_normal_chrome_and_non_browser_does_not_leak_url_or_title() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_incognito_no_leak.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    sm.handle_window_change(
        Some(WindowInfo::new("google-chrome", "GitHub - Google Chrome")),
        100.0,
    )
    .await
    .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://github.com/rust-lang/rust".to_string()),
            Some("GitHub".to_string()),
        ),
        100.4,
    )
    .await
    .unwrap();

    sm.handle_window_change(Some(WindowInfo::new("kitty", "zsh")), 105.0)
        .await
        .unwrap();

    let incognito_id = sm
        .handle_window_change(
            Some(WindowInfo::new(
                "google-chrome",
                "New Incognito Tab - Google Chrome",
            )),
            110.0,
        )
        .await
        .unwrap()
        .unwrap();

    let incognito_row = db.get_session(incognito_id).await.unwrap().unwrap();
    assert_eq!(incognito_row.website_url, None);
    assert_eq!(
        incognito_row.app_title.as_deref(),
        Some("New Incognito Tab - Google Chrome")
    );
}

#[tokio::test]
async fn test_switching_between_two_chrome_windows_updates_in_place_without_thrashing() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("test_two_chrome_windows.db");
    let db = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let mut sm = SessionManager::new(db.clone());

    sm.handle_window_change(
        Some(WindowInfo::new("google-chrome", "Window A - Google Chrome")),
        100.0,
    )
    .await
    .unwrap();
    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://docs.rs".to_string()),
            Some("Docs.rs".to_string()),
        ),
        100.3,
    )
    .await
    .unwrap();

    let win_b_id = sm
        .handle_window_change(
            Some(WindowInfo::new("google-chrome", "Window B - Google Chrome")),
            105.0,
        )
        .await
        .unwrap()
        .unwrap();

    sm.handle_url_update(
        UrlUpdate::new(
            "chrome",
            Some("https://crates.io".to_string()),
            Some("crates.io".to_string()),
        ),
        105.3,
    )
    .await
    .unwrap();

    assert_eq!(sm.current_session_id(), Some(win_b_id));
    let win_b_row = db.get_session(win_b_id).await.unwrap().unwrap();
    assert_eq!(win_b_row.website_url.as_deref(), Some("https://crates.io"));
    assert_eq!(win_b_row.app_title.as_deref(), Some("crates.io"));
}
