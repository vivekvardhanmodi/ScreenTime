#[cfg(target_os = "linux")]
mod common;

use async_trait::async_trait;
use screentime_core::{
    DbActorHandle, IdleDetector, IdleState, TrackerError, WindowInfo, WindowTracker,
};
use screentimed::sleep::{has_external_display_in, is_internal_connector, SleepEvent};
#[cfg(target_os = "linux")]
use screentimed::LogindSleepListener;
use screentimed::SessionManager;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;

#[test]
fn test_external_display_connector_classification() {
    assert!(is_internal_connector("card0-eDP-1"));
    assert!(is_internal_connector("card1-eDP-2"));
    assert!(is_internal_connector("card0-LVDS-1"));
    assert!(is_internal_connector("card0-DSI-1"));

    assert!(!is_internal_connector("card0-HDMI-A-1"));
    assert!(!is_internal_connector("card1-DP-1"));
    assert!(!is_internal_connector("card0-DVI-D-1"));
    assert!(!is_internal_connector("card0-VGA-1"));
    assert!(!is_internal_connector("card0-Virtual-1"));
}

#[test]
fn test_external_display_sysfs_detection() {
    let temp = tempdir().expect("create tempdir");
    let drm = temp.path();

    assert!(!has_external_display_in(drm));

    let edp_dir = drm.join("card1-eDP-1");
    std::fs::create_dir_all(&edp_dir).unwrap();
    std::fs::write(edp_dir.join("status"), "connected\n").unwrap();
    assert!(!has_external_display_in(drm));

    let hdmi_dir = drm.join("card1-HDMI-A-1");
    std::fs::create_dir_all(&hdmi_dir).unwrap();
    std::fs::write(hdmi_dir.join("status"), "disconnected\n").unwrap();
    assert!(!has_external_display_in(drm));

    std::fs::write(hdmi_dir.join("status"), "connected\n").unwrap();
    assert!(has_external_display_in(drm));

    std::fs::write(hdmi_dir.join("status"), "disconnected\n").unwrap();
    let dp_dir = drm.join("card1-DP-2");
    std::fs::create_dir_all(&dp_dir).unwrap();
    std::fs::write(dp_dir.join("status"), "connected\n").unwrap();
    assert!(has_external_display_in(drm));
}

#[tokio::test]
async fn test_session_manager_sleep_and_wake_lifecycle() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("screentime.db");
    let db = DbActorHandle::spawn(&db_path).expect("spawn db actor");

    let mut sm = SessionManager::new(db.clone());

    let win = WindowInfo {
        app_class: "Alacritty".to_string(),
        title: "Terminal".to_string(),
    };

    let session_id = sm
        .handle_window_change(Some(win.clone()), 100.0)
        .await
        .unwrap()
        .expect("active session started");
    assert_eq!(sm.current_session_id(), Some(session_id));
    assert!(sm.is_tracking_active());
    assert!(!sm.is_sleeping());

    sm.handle_sleep_event(SleepEvent::Sleep, 110.0)
        .await
        .unwrap();
    assert!(sm.is_sleeping());
    assert!(!sm.is_tracking_active());
    assert_eq!(sm.current_session_id(), None);

    let closed_session = db.get_session(session_id).await.unwrap().unwrap();
    assert_eq!(closed_session.start_time, 100.0);
    assert_eq!(closed_session.end_time, 110.0);
    assert_eq!(closed_session.duration(), 10.0);

    sm.handle_sleep_event(SleepEvent::Wake, 150.0)
        .await
        .unwrap();
    assert!(!sm.is_sleeping());
    assert!(sm.is_tracking_active());

    let wake_id = sm
        .current_session_id()
        .expect("new session started after wake");
    assert_ne!(wake_id, session_id);

    let wake_session = db.get_session(wake_id).await.unwrap().unwrap();
    assert_eq!(wake_session.start_time, 150.0);
    assert_eq!(wake_session.end_time, 150.0);
    assert_eq!(wake_session.app_class, "Alacritty");
}

#[tokio::test]
async fn test_session_manager_short_suspend_eliminates_time_leakage() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("screentime.db");
    let db = DbActorHandle::spawn(&db_path).expect("spawn db actor");

    let mut sm = SessionManager::new(db.clone());

    let win = WindowInfo {
        app_class: "firefox".to_string(),
        title: "Rust Documentation".to_string(),
    };

    let initial_id = sm
        .handle_window_change(Some(win.clone()), 1000.0)
        .await
        .unwrap()
        .unwrap();

    sm.handle_sleep_event(SleepEvent::Sleep, 1010.0)
        .await
        .unwrap();

    sm.handle_sleep_event(SleepEvent::Wake, 1025.0)
        .await
        .unwrap();

    let resumed_id = sm.current_session_id().unwrap();
    assert_ne!(resumed_id, initial_id);

    sm.handle_window_change(None, 1035.0).await.unwrap();

    let s1 = db.get_session(initial_id).await.unwrap().unwrap();
    assert_eq!(s1.start_time, 1000.0);
    assert_eq!(s1.end_time, 1010.0);
    assert_eq!(s1.duration(), 10.0);

    let s2 = db.get_session(resumed_id).await.unwrap().unwrap();
    assert_eq!(s2.start_time, 1025.0);
    assert_eq!(s2.end_time, 1035.0);
    assert_eq!(s2.duration(), 10.0);

    let total_sessions = db.get_sessions_in_range(0.0, 1e12).await.unwrap();
    assert_eq!(total_sessions.len(), 2);
    let total_time: f64 = total_sessions.iter().map(|s| s.duration()).sum();
    assert_eq!(total_time, 20.0);
}

#[tokio::test]
async fn test_session_manager_clamshell_lid_handling() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("screentime.db");
    let db = DbActorHandle::spawn(&db_path).expect("spawn db actor");

    let mut sm = SessionManager::new(db.clone());

    let win = WindowInfo {
        app_class: "code".to_string(),
        title: "screentimed".to_string(),
    };

    let initial_id = sm
        .handle_window_change(Some(win.clone()), 200.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sm.current_session_id(), Some(initial_id));

    sm.handle_sleep_event(
        SleepEvent::LidClosed {
            has_external_display: false,
        },
        210.0,
    )
    .await
    .unwrap();

    assert!(sm.is_lid_closed());
    assert!(!sm.has_external_display());
    assert!(!sm.is_tracking_active());
    assert_eq!(sm.current_session_id(), None);

    let s1 = db.get_session(initial_id).await.unwrap().unwrap();
    assert_eq!(s1.end_time, 210.0);

    sm.handle_sleep_event(SleepEvent::LidOpened, 220.0)
        .await
        .unwrap();

    assert!(!sm.is_lid_closed());
    assert!(sm.is_tracking_active());
    let reopened_id = sm.current_session_id().unwrap();
    assert_ne!(reopened_id, initial_id);

    sm.handle_sleep_event(
        SleepEvent::LidClosed {
            has_external_display: true,
        },
        230.0,
    )
    .await
    .unwrap();

    assert!(sm.is_lid_closed());
    assert!(sm.has_external_display());
    assert!(sm.is_tracking_active());
    assert_eq!(sm.current_session_id(), Some(reopened_id));
}

struct MockTracker {
    window: Option<WindowInfo>,
}

#[async_trait]
impl WindowTracker for MockTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        let _ = tx.send(self.window.clone()).await;
        Ok(())
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        Ok(self.window.clone())
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        Ok(())
    }
}

struct MockIdle;

#[async_trait]
impl IdleDetector for MockIdle {
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
async fn test_session_manager_run_loop_with_sleep_channel() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("screentime.db");
    let db = DbActorHandle::spawn(&db_path).expect("spawn db actor");

    let (sleep_tx, sleep_rx) = mpsc::channel(16);
    let (_url_tx, url_rx) = mpsc::channel::<screentime_core::ipc::UrlUpdate>(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let tracker = MockTracker {
        window: Some(WindowInfo {
            app_class: "terminal".to_string(),
            title: "bash".to_string(),
        }),
    };
    let idle = MockIdle;

    let sm = SessionManager::new(db.clone());
    let db_clone = db.clone();

    let runner = tokio::spawn(async move {
        sm.run_loop_with_sleep(tracker, idle, url_rx, sleep_rx, shutdown_rx)
            .await
    });

    tokio::time::sleep(Duration::from_millis(1100)).await;

    let initial_sessions = db_clone.get_sessions_in_range(0.0, 1e12).await.unwrap();
    assert_eq!(initial_sessions.len(), 1, "Initial session should exist");
    let s1_id = initial_sessions[0].id.unwrap();

    sleep_tx.send(SleepEvent::Sleep).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let s1_closed = db_clone.get_session(s1_id).await.unwrap().unwrap();
    assert!(s1_closed.end_time >= s1_closed.start_time);

    sleep_tx.send(SleepEvent::Wake).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let wake_sessions = db_clone.get_sessions_in_range(0.0, 1e12).await.unwrap();
    assert_eq!(wake_sessions.len(), 2, "Session should resume upon wake");
    let s2_id = wake_sessions.iter().map(|s| s.id.unwrap()).max().unwrap();
    assert_ne!(s1_id, s2_id);

    shutdown_tx.send(()).await.unwrap();
    let res = runner.await.unwrap();
    assert!(res.is_ok(), "run_loop should exit cleanly on shutdown");
}

#[cfg(target_os = "linux")]
mod linux_dbus_tests {
    use super::common::TestDbus;
    use super::*;
    use zbus::connection::Builder;
    use zbus::interface;

    struct MockLogindManager {
        lid_closed: Arc<AtomicBool>,
    }

    #[interface(name = "org.freedesktop.login1.Manager")]
    impl MockLogindManager {
        #[zbus(signal)]
        async fn prepare_for_sleep(
            signal_emitter: &zbus::object_server::SignalEmitter<'_>,
            start: bool,
        ) -> zbus::Result<()>;

        #[zbus(property)]
        async fn lid_closed(&self) -> bool {
            self.lid_closed.load(Ordering::SeqCst)
        }

        #[zbus(property)]
        async fn set_lid_closed(
            &self,
            #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
            value: bool,
        ) -> zbus::Result<()> {
            self.lid_closed.store(value, Ordering::SeqCst);
            self.lid_closed_changed(&emitter).await
        }
    }

    #[tokio::test]
    async fn test_logind_sleep_listener_signals_and_properties() {
        let bus = TestDbus::spawn();
        let service_name = "org.freedesktop.login1";
        let object_path = "/org/freedesktop/login1";

        let server_lid = Arc::new(AtomicBool::new(false));
        let mock_server = MockLogindManager {
            lid_closed: server_lid.clone(),
        };

        let server_conn = Builder::address(bus.address())
            .unwrap()
            .name(service_name)
            .unwrap()
            .serve_at(object_path, mock_server)
            .unwrap()
            .build()
            .await
            .unwrap();

        let client_conn = Builder::address(bus.address())
            .unwrap()
            .build()
            .await
            .unwrap();

        let (sleep_tx, mut sleep_rx) = mpsc::channel(16);

        let drm_temp = tempdir().unwrap();
        let drm_path = drm_temp.path().to_path_buf();
        let edp = drm_path.join("card0-eDP-1");
        std::fs::create_dir_all(&edp).unwrap();
        std::fs::write(edp.join("status"), "connected\n").unwrap();

        let mut listener = LogindSleepListener::start_with_options(
            client_conn,
            service_name,
            object_path,
            sleep_tx,
            Some(drm_path.clone()),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let emitter = zbus::object_server::SignalEmitter::new(&server_conn, object_path).unwrap();
        MockLogindManager::prepare_for_sleep(&emitter, true)
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), sleep_rx.recv())
            .await
            .expect("timeout waiting for PrepareForSleep(true)")
            .expect("event");
        assert_eq!(event, SleepEvent::Sleep);

        MockLogindManager::prepare_for_sleep(&emitter, false)
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), sleep_rx.recv())
            .await
            .expect("timeout waiting for PrepareForSleep(false)")
            .expect("event");
        assert_eq!(event, SleepEvent::Wake);

        let iface_ref = server_conn
            .object_server()
            .interface::<_, MockLogindManager>(object_path)
            .await
            .unwrap();
        iface_ref
            .get_mut()
            .await
            .set_lid_closed(iface_ref.signal_emitter().clone(), true)
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), sleep_rx.recv())
            .await
            .expect("timeout waiting for LidClosed(true)")
            .expect("event");
        assert_eq!(
            event,
            SleepEvent::LidClosed {
                has_external_display: false
            }
        );

        iface_ref
            .get_mut()
            .await
            .set_lid_closed(iface_ref.signal_emitter().clone(), false)
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), sleep_rx.recv())
            .await
            .expect("timeout waiting for LidClosed(false)")
            .expect("event");
        assert_eq!(event, SleepEvent::LidOpened);

        listener.stop().await.unwrap();
    }
}
