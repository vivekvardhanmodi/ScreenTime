use async_trait::async_trait;
use screentime_core::{
    DbActorHandle, IdleDetector, IdleState, TrackerError, WindowInfo, WindowTracker,
};
use screentimed::kde::{
    generate_kwin_script, parse_window_changed, resolve_kwin_script_path, KdeWindowTracker,
    DEFAULT_KWIN_SCRIPT,
};
use screentimed::SessionManager;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::{mpsc, Mutex};
use zbus::connection::Builder;
use zbus::interface;

struct TestDbus {
    _temp_dir: tempfile::TempDir,
    address: String,
    process: std::process::Child,
}

impl TestDbus {
    fn spawn() -> Self {
        let temp_dir = tempdir().expect("create tempdir");
        let sock_path = temp_dir.path().join("bus.sock");
        let address = format!("unix:path={}", sock_path.display());

        let process = std::process::Command::new("dbus-daemon")
            .arg("--session")
            .arg(format!("--address={address}"))
            .arg("--nofork")
            .arg("--nopidfile")
            .spawn()
            .expect("spawn dbus-daemon");

        for _ in 0..100 {
            if sock_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(sock_path.exists(), "dbus socket must exist");

        Self {
            _temp_dir: temp_dir,
            address,
            process,
        }
    }

    fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for TestDbus {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

struct MockKWinScripting {
    loaded_scripts: Arc<Mutex<Vec<(String, String)>>>,
    unloaded_scripts: Arc<Mutex<Vec<String>>>,
    script_runs: Arc<Mutex<usize>>,
    conn: zbus::Connection,
}

#[interface(name = "org.kde.kwin.Scripting")]
impl MockKWinScripting {
    async fn load_script(&self, file_path: String, plugin_name: String) -> i32 {
        self.loaded_scripts
            .lock()
            .await
            .push((file_path, plugin_name));
        let script_id = 42;
        let mock_script = MockKWinScript {
            script_runs: Arc::clone(&self.script_runs),
        };
        let _ = self
            .conn
            .object_server()
            .at("/Scripting/Script42", mock_script)
            .await;
        script_id
    }

    async fn unload_script(&self, plugin_name: String) -> bool {
        self.unloaded_scripts.lock().await.push(plugin_name);
        true
    }

    async fn start(&self) {
        *self.script_runs.lock().await += 1;
    }
}

struct MockKWinScript {
    script_runs: Arc<Mutex<usize>>,
}

#[interface(name = "org.kde.kwin.Script")]
impl MockKWinScript {
    async fn run(&self) {
        *self.script_runs.lock().await += 1;
    }

    async fn stop(&self) {}
}

struct DummyIdleDetector;

#[async_trait]
impl IdleDetector for DummyIdleDetector {
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

#[test]
fn test_kwin_script_generation_and_defaults() {
    let generated = generate_kwin_script(
        "org.screentime.Daemon",
        "/org/screentime/Tracker",
        "org.screentime.Tracker",
    );
    assert_eq!(DEFAULT_KWIN_SCRIPT, generated);

    assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.windowActivated"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.clientActivated"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.activeWindow"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.activeClient"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("captionChanged"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("callDBus"));
    assert!(DEFAULT_KWIN_SCRIPT.contains("WindowChanged"));

    let custom = generate_kwin_script(
        "org.custom.Daemon",
        "/org/custom/Path",
        "org.custom.Interface",
    );
    assert!(custom.contains("\"org.custom.Daemon\""));
    assert!(custom.contains("\"/org/custom/Path\""));
    assert!(custom.contains("\"org.custom.Interface\""));
}

#[test]
fn test_parse_window_changed_unit() {
    let normal = parse_window_changed("kitty", "fish shell").unwrap();
    assert_eq!(normal.app_class, "kitty");
    assert_eq!(normal.title, "fish shell");

    let comma_title =
        parse_window_changed("firefox", "Issue #5: KDE Tracking, Ready - Mozilla Firefox").unwrap();
    assert_eq!(comma_title.app_class, "firefox");
    assert_eq!(
        comma_title.title,
        "Issue #5: KDE Tracking, Ready - Mozilla Firefox"
    );

    let trimmed = parse_window_changed("  alacritty  ", "  editor  ").unwrap();
    assert_eq!(trimmed.app_class, "alacritty");
    assert_eq!(trimmed.title, "editor");

    assert_eq!(parse_window_changed("", ""), None);
    assert_eq!(parse_window_changed("   ", "   "), None);
    assert_eq!(parse_window_changed("", "Some Title"), None);
    assert_eq!(parse_window_changed("   ", "Some Title"), None);

    let empty_title = parse_window_changed("code", "").unwrap();
    assert_eq!(empty_title.app_class, "code");
    assert_eq!(empty_title.title, "");
}

#[test]
fn test_resolve_kwin_script_path() {
    let custom = Path::new("/tmp/screentime_custom_test");
    let p1 = resolve_kwin_script_path(Some(custom), Some("custom_tracker.js"));
    assert_eq!(p1, custom.join("custom_tracker.js"));

    let p2 = resolve_kwin_script_path(Some(custom), None);
    assert_eq!(p2, custom.join("kwin_tracker.js"));
}

#[tokio::test]
async fn test_kde_tracker_script_lifecycle_ephemeral_dir() {
    let bus = TestDbus::spawn();
    let temp_runtime = tempdir().unwrap();

    let mut tracker = KdeWindowTracker::new()
        .with_bus_address(bus.address())
        .with_runtime_dir(temp_runtime.path())
        .with_script_filename("test_kwin.js")
        .with_plugin_name("test_plugin");

    let (tx, _rx) = mpsc::channel(16);
    tracker.start(tx).await.unwrap();

    let expected_script_path = temp_runtime.path().join("test_kwin.js");
    assert!(expected_script_path.exists(), "Script file must be written");

    let content = tokio::fs::read_to_string(&expected_script_path)
        .await
        .unwrap();
    assert!(content.contains("workspace.windowActivated"));
    assert!(content.contains("WindowChanged"));

    tracker.stop().await.unwrap();
    assert!(
        !expected_script_path.exists(),
        "Script file must be removed on stop"
    );
}

#[tokio::test]
async fn test_mock_kwin_scripting_load_run_unload_and_window_changed() {
    let bus = TestDbus::spawn();
    let temp_runtime = tempdir().unwrap();

    let kwin_conn = Builder::address(bus.address())
        .unwrap()
        .build()
        .await
        .unwrap();

    let loaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let unloaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let script_runs = Arc::new(Mutex::new(0));

    let mock_scripting = MockKWinScripting {
        loaded_scripts: Arc::clone(&loaded_scripts),
        unloaded_scripts: Arc::clone(&unloaded_scripts),
        script_runs: Arc::clone(&script_runs),
        conn: kwin_conn.clone(),
    };

    kwin_conn
        .object_server()
        .at("/Scripting", mock_scripting)
        .await
        .unwrap();
    kwin_conn.request_name("org.kde.KWin").await.unwrap();

    let mut tracker = KdeWindowTracker::new()
        .with_bus_address(bus.address())
        .with_runtime_dir(temp_runtime.path())
        .with_plugin_name("screentime_tracker");

    let (tx, mut rx) = mpsc::channel(16);
    tracker.start(tx).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let loaded = loaded_scripts.lock().await;
        assert_eq!(loaded.len(), 1, "loadScript must have been called once");
        assert_eq!(loaded[0].1, "screentime_tracker");
        assert!(Path::new(&loaded[0].0).exists());
    }

    {
        let runs = *script_runs.lock().await;
        assert_eq!(runs, 1, "Script .run() must have been called");
    }

    let daemon_proxy = zbus::Proxy::new(
        &kwin_conn,
        "org.screentime.Daemon",
        "/org/screentime/Tracker",
        "org.screentime.Tracker",
    )
    .await
    .unwrap();

    let _: () = daemon_proxy
        .call(
            "WindowChanged",
            &("firefox", "Issue #5: KDE - Mozilla Firefox"),
        )
        .await
        .unwrap();

    let ev1 = rx.recv().await.unwrap();
    assert_eq!(
        ev1,
        Some(WindowInfo::new(
            "firefox",
            "Issue #5: KDE - Mozilla Firefox"
        ))
    );

    let active = tracker.query_active_window().await.unwrap();
    assert_eq!(
        active,
        Some(WindowInfo::new(
            "firefox",
            "Issue #5: KDE - Mozilla Firefox"
        ))
    );

    let _: () = daemon_proxy.call("WindowChanged", &("", "")).await.unwrap();

    let ev2 = rx.recv().await.unwrap();
    assert_eq!(ev2, None);

    let active_none = tracker.query_active_window().await.unwrap();
    assert_eq!(active_none, None);

    let _: () = daemon_proxy
        .call("WindowChanged", &("kitty", "terminal"))
        .await
        .unwrap();

    let ev3 = rx.recv().await.unwrap();
    assert_eq!(ev3, Some(WindowInfo::new("kitty", "terminal")));

    tracker.stop().await.unwrap();
    {
        let unloaded = unloaded_scripts.lock().await;
        assert!(
            unloaded.contains(&"screentime_tracker".to_string()),
            "unloadScript must have been called for screentime_tracker"
        );
    }
}

#[tokio::test]
async fn test_kwin_restart_resilience_name_owner_changed() {
    let bus = TestDbus::spawn();
    let temp_runtime = tempdir().unwrap();

    let kwin_conn1 = Builder::address(bus.address())
        .unwrap()
        .build()
        .await
        .unwrap();

    let loaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let unloaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let script_runs = Arc::new(Mutex::new(0));

    let mock_scripting = MockKWinScripting {
        loaded_scripts: Arc::clone(&loaded_scripts),
        unloaded_scripts: Arc::clone(&unloaded_scripts),
        script_runs: Arc::clone(&script_runs),
        conn: kwin_conn1.clone(),
    };

    kwin_conn1
        .object_server()
        .at("/Scripting", mock_scripting)
        .await
        .unwrap();
    kwin_conn1.request_name("org.kde.KWin").await.unwrap();

    let mut tracker = KdeWindowTracker::new()
        .with_bus_address(bus.address())
        .with_runtime_dir(temp_runtime.path())
        .with_plugin_name("screentime_tracker");

    let (tx, _rx) = mpsc::channel(16);
    tracker.start(tx).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(loaded_scripts.lock().await.len(), 1);

    kwin_conn1.release_name("org.kde.KWin").await.unwrap();

    let kwin_conn2 = Builder::address(bus.address())
        .unwrap()
        .build()
        .await
        .unwrap();

    let mock_scripting2 = MockKWinScripting {
        loaded_scripts: Arc::clone(&loaded_scripts),
        unloaded_scripts: Arc::clone(&unloaded_scripts),
        script_runs: Arc::clone(&script_runs),
        conn: kwin_conn2.clone(),
    };

    kwin_conn2
        .object_server()
        .at("/Scripting", mock_scripting2)
        .await
        .unwrap();

    kwin_conn2.request_name("org.kde.KWin").await.unwrap();

    tokio::time::sleep(Duration::from_millis(800)).await;

    {
        let loaded = loaded_scripts.lock().await;
        assert_eq!(
            loaded.len(),
            2,
            "Tracker must have re-loaded script into restarted KWin"
        );
    }

    tracker.stop().await.unwrap();
}

#[tokio::test]
async fn test_kde_tracker_with_session_manager_integration() {
    let temp_db_dir = tempdir().unwrap();
    let db_path = temp_db_dir.path().join("test_kde_screentime.db");
    let db_handle = DbActorHandle::spawn(&db_path).unwrap();

    let bus = TestDbus::spawn();
    let temp_runtime = tempdir().unwrap();

    let kwin_conn = Builder::address(bus.address())
        .unwrap()
        .build()
        .await
        .unwrap();

    let loaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let unloaded_scripts = Arc::new(Mutex::new(Vec::new()));
    let script_runs = Arc::new(Mutex::new(0));

    let mock_scripting = MockKWinScripting {
        loaded_scripts: Arc::clone(&loaded_scripts),
        unloaded_scripts: Arc::clone(&unloaded_scripts),
        script_runs: Arc::clone(&script_runs),
        conn: kwin_conn.clone(),
    };

    kwin_conn
        .object_server()
        .at("/Scripting", mock_scripting)
        .await
        .unwrap();
    kwin_conn.request_name("org.kde.KWin").await.unwrap();

    let tracker = KdeWindowTracker::new()
        .with_bus_address(bus.address())
        .with_runtime_dir(temp_runtime.path())
        .with_plugin_name("screentime_tracker");

    let idle = DummyIdleDetector;

    let manager = SessionManager::new(db_handle.clone()).with_device_id("test-kde-pc");
    let (url_tx, url_rx) = mpsc::channel(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let session_loop = tokio::spawn(async move {
        manager
            .run_loop(tracker, idle, url_rx, shutdown_rx)
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let daemon_proxy = zbus::Proxy::new(
        &kwin_conn,
        "org.screentime.Daemon",
        "/org/screentime/Tracker",
        "org.screentime.Tracker",
    )
    .await
    .unwrap();

    let _: () = daemon_proxy
        .call(
            "WindowChanged",
            &("firefox", "Rust Documentation - Mozilla Firefox"),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    url_tx
        .send("https://doc.rust-lang.org".to_string())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let _: () = daemon_proxy
        .call("WindowChanged", &("alacritty", "cargo test"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;

    shutdown_tx.send(()).await.unwrap();
    session_loop.await.unwrap();

    let history = db_handle
        .get_sessions_in_range(0.0, f64::MAX)
        .await
        .unwrap();
    assert!(
        !history.is_empty(),
        "Should have recorded sessions in ephemeral database"
    );
    assert!(
        history
            .iter()
            .any(|s| s.app_class == "firefox" || s.app_class == "alacritty"),
        "Sessions should include firefox or alacritty"
    );
}
