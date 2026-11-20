use screentimed::sleep;
#[path = "../src/windows/mod.rs"]
mod windows;

use screentime_core::{DbActorHandle, IdleDetector, IdleState, WindowInfo, WindowTracker};
use screentimed::SessionManager;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use windows::events::{
    clean_window_title, decode_wide_string, parse_display_power_state, parse_session_change_event,
    PlatformEvent, WTS_SESSION_LOCK, WTS_SESSION_LOGOFF, WTS_SESSION_UNLOCK,
};
use windows::math::{
    calculate_idle_duration_ms, calculate_idle_time, calculate_poll_interval,
    evaluate_idle_transition,
};
use windows::uwp::{
    extract_process_name, find_uwp_child_process, is_uwp_container, ChildWindowCandidate,
    UWP_CORE_WINDOW_CLASS,
};
use windows::{WindowsIdleDetector, WindowsTracker};

#[test]
fn test_windows_idle_tick_rollover_math() {
    // Normal case (no rollover): current > last
    assert_eq!(calculate_idle_duration_ms(100_000, 95_000), 5_000);
    assert_eq!(calculate_idle_duration_ms(100_000, 100_000), 0);
    assert_eq!(
        calculate_idle_time(50_000, 20_000),
        Duration::from_millis(30_000)
    );

    // 49.7-day rollover boundary:
    // last_input occurred 16 ms before u32::MAX (0xFFFF_FFF0)
    // current_ticks is 32 ms after 0 (0x0000_0020)
    // Elapsed = 16 + 32 = 48 ms
    let last_input = 0xFFFF_FFF0_u32;
    let current = 0x0000_0020_u32;
    assert_eq!(calculate_idle_duration_ms(current, last_input), 48);
    assert_eq!(
        calculate_idle_time(current, last_input),
        Duration::from_millis(48)
    );

    // Exact boundary at u32::MAX
    assert_eq!(calculate_idle_duration_ms(0, u32::MAX), 1);
    assert_eq!(calculate_idle_duration_ms(500, u32::MAX), 501);

    // Reverse check: identical ticks produce zero
    assert_eq!(calculate_idle_duration_ms(u32::MAX, u32::MAX), 0);
    assert_eq!(calculate_idle_duration_ms(0, 0), 0);
}

#[test]
fn test_windows_adaptive_polling_intervals() {
    let threshold = Duration::from_secs(90);

    // When idle: poll every 1.0s to detect resume immediately
    assert_eq!(
        calculate_poll_interval(true, Duration::from_secs(120), threshold),
        Duration::from_millis(1000)
    );
    assert_eq!(
        calculate_poll_interval(true, Duration::from_secs(0), threshold),
        Duration::from_millis(1000)
    );

    // When active and far from threshold (e.g. 10s into 90s timeout): poll every 2.0s
    assert_eq!(
        calculate_poll_interval(false, Duration::from_secs(10), threshold),
        Duration::from_millis(2000)
    );

    // When active and near threshold (within 5s of 90s timeout, e.g. 86s): poll every 1.0s
    assert_eq!(
        calculate_poll_interval(false, Duration::from_secs(85), threshold),
        Duration::from_millis(1000)
    );
    assert_eq!(
        calculate_poll_interval(false, Duration::from_secs(89), threshold),
        Duration::from_millis(1000)
    );
}

#[test]
fn test_windows_idle_state_transitions() {
    let idle_threshold = Duration::from_secs(60);
    let resume_threshold = Duration::from_millis(1500);

    // Active state staying active below threshold
    assert_eq!(
        evaluate_idle_transition(
            IdleState::Active,
            Duration::from_secs(59),
            idle_threshold,
            resume_threshold
        ),
        IdleState::Active
    );

    // Active state crossing into Idle at threshold
    assert_eq!(
        evaluate_idle_transition(
            IdleState::Active,
            Duration::from_secs(60),
            idle_threshold,
            resume_threshold
        ),
        IdleState::Idle
    );

    // Idle state remaining Idle
    assert_eq!(
        evaluate_idle_transition(
            IdleState::Idle,
            Duration::from_secs(90),
            idle_threshold,
            resume_threshold
        ),
        IdleState::Idle
    );

    // Idle state resuming to Active upon user input (idle duration < 1500ms)
    assert_eq!(
        evaluate_idle_transition(
            IdleState::Idle,
            Duration::from_millis(500),
            idle_threshold,
            resume_threshold
        ),
        IdleState::Active
    );
}

#[test]
fn test_windows_process_path_extraction() {
    // Windows paths with backslashes
    assert_eq!(
        extract_process_name(
            r"C:\Program Files\WindowsApps\Microsoft.WindowsCalculator_8wekyb3d8bbwe\CalculatorApp.exe"
        ),
        "CalculatorApp.exe"
    );
    assert_eq!(
        extract_process_name(r"C:\Windows\System32\notepad.exe"),
        "notepad.exe"
    );
    assert_eq!(
        extract_process_name(
            r"\Device\HarddiskVolume3\Program Files\Google\Chrome\Application\chrome.exe"
        ),
        "chrome.exe"
    );

    // Mixed and forward slashes
    assert_eq!(
        extract_process_name("C:/Program Files/Sublime Text/sublime_text.exe"),
        "sublime_text.exe"
    );
    assert_eq!(extract_process_name("/usr/bin/alacritty"), "alacritty");

    // Standalone filename
    assert_eq!(extract_process_name("code.exe"), "code.exe");

    // Whitespace and empty strings
    assert_eq!(extract_process_name("   code.exe   "), "code.exe");
    assert_eq!(extract_process_name(""), "");
    assert_eq!(extract_process_name("   "), "");
}

#[test]
fn test_windows_is_uwp_container() {
    assert!(is_uwp_container("ApplicationFrameHost.exe"));
    assert!(is_uwp_container("applicationframehost.exe"));
    assert!(is_uwp_container("APPLICATIONFRAMEHOST.EXE"));
    assert!(is_uwp_container("ApplicationFrameHost.EXE"));

    assert!(!is_uwp_container("CalculatorApp.exe"));
    assert!(!is_uwp_container("explorer.exe"));
    assert!(!is_uwp_container("cmd.exe"));
    assert!(!is_uwp_container(""));
}

#[test]
fn test_windows_uwp_child_window_resolution() {
    let host_pid = 4000;

    let candidates = vec![
        ChildWindowCandidate::new(0x10, host_pid, UWP_CORE_WINDOW_CLASS), // host PID, rejected
        ChildWindowCandidate::new(0x11, 5001, "IntermediateD3DWindow"),   // wrong class, rejected
        ChildWindowCandidate::new(0x12, 0, UWP_CORE_WINDOW_CLASS),        // PID 0, rejected
        ChildWindowCandidate::new(0x13, 6002, UWP_CORE_WINDOW_CLASS),     // Match!
        ChildWindowCandidate::new(0x14, 7003, UWP_CORE_WINDOW_CLASS),     // Secondary
    ];

    let resolved = find_uwp_child_process(host_pid, &candidates);
    assert_eq!(resolved, Some((0x13, 6002)));

    // Fallback when no child matches
    let no_match = vec![
        ChildWindowCandidate::new(0x10, host_pid, UWP_CORE_WINDOW_CLASS),
        ChildWindowCandidate::new(0x11, 5001, "WorkerW"),
    ];
    assert_eq!(find_uwp_child_process(host_pid, &no_match), None);
}

#[test]
fn test_windows_window_title_sanitization() {
    assert_eq!(
        clean_window_title("  GitHub - Screentime  "),
        "GitHub - Screentime"
    );
    assert_eq!(clean_window_title("\0Trailing null\0"), "Trailing null");
    assert_eq!(clean_window_title("   "), "");

    let raw_wide: Vec<u16> = "Visual Studio Code\0Extra Data".encode_utf16().collect();
    assert_eq!(decode_wide_string(&raw_wide), "Visual Studio Code");

    let no_null: Vec<u16> = "Terminal".encode_utf16().collect();
    assert_eq!(decode_wide_string(&no_null), "Terminal");
}

#[test]
fn test_windows_display_power_and_session_events() {
    // GUID_CONSOLE_DISPLAY_STATE data values
    assert_eq!(
        parse_display_power_state(0),
        Some(PlatformEvent::DisplayOff)
    );
    assert_eq!(parse_display_power_state(1), Some(PlatformEvent::DisplayOn));
    assert_eq!(
        parse_display_power_state(2),
        Some(PlatformEvent::DisplayDim)
    );
    assert_eq!(parse_display_power_state(99), None);

    // WTSRegisterSessionNotification wParam values
    assert_eq!(
        parse_session_change_event(WTS_SESSION_LOCK),
        Some(PlatformEvent::SessionLocked)
    );
    assert_eq!(
        parse_session_change_event(WTS_SESSION_UNLOCK),
        Some(PlatformEvent::SessionUnlocked)
    );
    assert_eq!(
        parse_session_change_event(WTS_SESSION_LOGOFF),
        Some(PlatformEvent::SessionLogoff)
    );
    assert_eq!(parse_session_change_event(999), None);
}

#[tokio::test]
async fn test_windows_tracker_event_streaming() {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let mut tracker = WindowsTracker::new().with_mock_receiver(event_rx);

    let (win_tx, mut win_rx) = mpsc::channel(16);
    tracker.start(win_tx).await.expect("tracker started");

    // Send focus changes
    event_tx
        .send(PlatformEvent::WindowChanged(Some(WindowInfo::new(
            "chrome.exe",
            "GitHub - Pull Requests",
        ))))
        .unwrap();

    let win = win_rx.recv().await.expect("received window event");
    assert_eq!(
        win,
        Some(WindowInfo::new("chrome.exe", "GitHub - Pull Requests"))
    );

    // Query active window should reflect updated state
    let active = tracker.query_active_window().await.unwrap();
    assert_eq!(
        active,
        Some(WindowInfo::new("chrome.exe", "GitHub - Pull Requests"))
    );

    // Send unfocused / desktop event
    event_tx.send(PlatformEvent::WindowChanged(None)).unwrap();

    let win_none = win_rx.recv().await.expect("received unfocused event");
    assert_eq!(win_none, None);

    // Clean shutdown
    tracker.stop().await.expect("tracker stopped");
}

#[tokio::test]
async fn test_windows_idle_detector_with_mock_events() {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let mut idle = WindowsIdleDetector::new(Duration::from_secs(60)).with_mock_receiver(event_rx);

    let (idle_tx, mut idle_rx) = mpsc::channel(16);
    idle.start(idle_tx).await.expect("idle detector started");

    assert!(!idle.is_idle());

    // 1. DisplayOff triggers immediate Idle
    event_tx.send(PlatformEvent::DisplayOff).unwrap();
    let state = idle_rx.recv().await.expect("received idle state");
    assert_eq!(state, IdleState::Idle);
    assert!(idle.is_idle());

    // 2. DisplayOn triggers immediate Active
    event_tx.send(PlatformEvent::DisplayOn).unwrap();
    let state = idle_rx.recv().await.expect("received active state");
    assert_eq!(state, IdleState::Active);
    assert!(!idle.is_idle());

    // 3. SessionLocked triggers immediate Idle
    event_tx.send(PlatformEvent::SessionLocked).unwrap();
    let state = idle_rx.recv().await.expect("received locked state");
    assert_eq!(state, IdleState::Idle);
    assert!(idle.is_idle());

    // 4. SessionUnlocked triggers immediate Active
    event_tx.send(PlatformEvent::SessionUnlocked).unwrap();
    let state = idle_rx.recv().await.expect("received unlocked state");
    assert_eq!(state, IdleState::Active);
    assert!(!idle.is_idle());

    idle.stop().await.expect("stopped cleanly");
}

#[tokio::test]
async fn test_windows_idle_detector_with_mock_ticks_and_rollover() {
    let current_ticks = Arc::new(AtomicU32::new(0xFFFF_FF00));
    let last_input_ticks = Arc::new(AtomicU32::new(0xFFFF_FF00));

    let cur_clone = current_ticks.clone();
    let last_clone = last_input_ticks.clone();

    let tick_fn = Arc::new(move || {
        (
            cur_clone.load(Ordering::SeqCst),
            last_clone.load(Ordering::SeqCst),
        )
    });

    // Short threshold (20ms) for test execution speed
    let mut idle = WindowsIdleDetector::new(Duration::from_millis(20))
        .with_resume_threshold(Duration::from_millis(5))
        .with_mock_tick_fn(tick_fn);

    let (idle_tx, mut idle_rx) = mpsc::channel(16);
    idle.start(idle_tx).await.expect("started");

    // Initially active (0ms idle)
    assert!(!idle.is_idle());

    // Advance current_ticks across the 49.7-day 32-bit rollover:
    // last_input = 0xFFFF_FF00, current = 0x0000_0050
    // Diff = 256 + 80 = 336 ms (> 20ms threshold)
    current_ticks.store(0x0000_0050, Ordering::SeqCst);

    let state = tokio::time::timeout(Duration::from_secs(3), idle_rx.recv())
        .await
        .expect("timed out waiting for idle transition")
        .expect("channel message");
    assert_eq!(state, IdleState::Idle);
    assert!(idle.is_idle());

    // User activity resumes: last_input updated to match current
    last_input_ticks.store(0x0000_0050, Ordering::SeqCst);

    let state2 = tokio::time::timeout(Duration::from_secs(3), idle_rx.recv())
        .await
        .expect("timed out waiting for active resume")
        .expect("channel message");
    assert_eq!(state2, IdleState::Active);
    assert!(!idle.is_idle());

    idle.stop().await.expect("stopped cleanly");
}

#[test]
fn test_windows_idle_detector_query_idle_time_and_builders() {
    let tick_fn = Arc::new(|| (5000, 2000));
    let idle = WindowsIdleDetector::new(Duration::from_secs(30))
        .with_idle_threshold(Duration::from_secs(45))
        .with_mock_tick_fn(tick_fn);

    assert_eq!(idle.query_idle_time(), Duration::from_millis(3000));
    assert!(!idle.is_idle());
}

#[tokio::test]
async fn test_windows_tracker_and_idle_with_session_manager() {
    let temp_dir = tempdir().expect("created temp dir");
    let db_path = temp_dir.path().join("screentime_test.db");

    let handle = DbActorHandle::spawn(&db_path).expect("started db actor");

    let (tracker_tx, tracker_rx) = mpsc::unbounded_channel();
    let tracker = WindowsTracker::new().with_mock_receiver(tracker_rx);

    let (idle_tx, idle_rx) = mpsc::unbounded_channel();
    let idle = WindowsIdleDetector::new(Duration::from_secs(60)).with_mock_receiver(idle_rx);

    let session_manager = SessionManager::new(handle.clone()).with_device_id("windows-pc");

    let (url_tx, url_rx) = mpsc::channel(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let run_handle = tokio::spawn(async move {
        session_manager
            .run_loop(tracker, idle, url_rx, shutdown_rx)
            .await
    });

    tracker_tx
        .send(PlatformEvent::WindowChanged(Some(WindowInfo::new(
            "chrome.exe",
            "GitHub - Pull Requests",
        ))))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    url_tx
        .send(screentime_core::ipc::UrlUpdate {
            url: Some("https://github.com/rust-lang/rust".to_string()),
            title: Some("GitHub - Pull Requests".to_string()),
            browser_type: "chrome".to_string(),
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1100)).await;

    tracker_tx
        .send(PlatformEvent::WindowChanged(Some(WindowInfo::new(
            "CalculatorApp.exe",
            "Calculator",
        ))))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1100)).await;

    idle_tx.send(PlatformEvent::DisplayOff).unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    idle_tx.send(PlatformEvent::SessionLocked).unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    idle_tx.send(PlatformEvent::SessionUnlocked).unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    idle_tx.send(PlatformEvent::DisplayOn).unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    shutdown_tx.send(()).await.unwrap();
    run_handle.await.unwrap().expect("run_loop exited cleanly");

    // Verify sessions recorded in ephemeral DB
    let history = handle
        .get_sessions_in_range(0.0, f64::MAX)
        .await
        .expect("fetched history");
    assert!(
        !history.is_empty(),
        "Expected sessions recorded during Windows tracking"
    );

    // Verify device_id is preserved
    for session in &history {
        assert_eq!(session.device_id, "windows-pc");
    }

    // Verify first session was chrome.exe with website URL
    let chrome_session = history
        .iter()
        .find(|s| s.app_class == "chrome.exe")
        .expect("Found chrome session");
    assert_eq!(
        chrome_session.website_url.as_deref(),
        Some("https://github.com/rust-lang/rust")
    );

    // Verify Calculator session exists
    assert!(history.iter().any(|s| s.app_class == "CalculatorApp.exe"));

    // Drop handle
    drop(handle);
}

#[tokio::test]
async fn test_windows_power_suspend_resume_with_session_manager() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("screentime.db");
    let db = DbActorHandle::spawn(&db_path).expect("spawn db actor");

    let (tracker_tx, tracker_rx) = mpsc::unbounded_channel::<PlatformEvent>();
    let (idle_event_tx, idle_event_rx) = mpsc::unbounded_channel::<PlatformEvent>();
    let (sleep_tx, sleep_rx) = mpsc::channel::<sleep::SleepEvent>(16);
    let (_url_tx, url_rx) = mpsc::channel::<screentime_core::ipc::UrlUpdate>(16);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let tracker = WindowsTracker::new().with_mock_receiver(tracker_rx);
    let idle = WindowsIdleDetector::default()
        .with_mock_receiver(idle_event_rx)
        .with_sleep_sender(sleep_tx);

    let sm = SessionManager::new(db.clone()).with_device_id("windows-test-pc");
    let db_clone = db.clone();

    let run_handle = tokio::spawn(async move {
        sm.run_loop_with_sleep(tracker, idle, url_rx, sleep_rx, shutdown_rx)
            .await
    });

    tracker_tx
        .send(PlatformEvent::WindowChanged(Some(WindowInfo {
            app_class: "notepad.exe".to_string(),
            title: "Untitled - Notepad".to_string(),
        })))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1100)).await;

    let initial_sessions = db_clone.get_sessions_in_range(0.0, 1e12).await.unwrap();
    assert_eq!(initial_sessions.len(), 1);
    let s1_id = initial_sessions[0].id.unwrap();

    idle_event_tx.send(PlatformEvent::Suspend).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let s1 = db_clone.get_session(s1_id).await.unwrap().unwrap();
    assert!(s1.end_time >= s1.start_time);

    idle_event_tx.send(PlatformEvent::Resume).unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let resumed_sessions = db_clone.get_sessions_in_range(0.0, 1e12).await.unwrap();
    assert_eq!(resumed_sessions.len(), 2, "New session created upon wake");
    let s2_id = resumed_sessions.iter().map(|s| s.id.unwrap()).max().unwrap();
    assert_ne!(s1_id, s2_id);

    shutdown_tx.send(()).await.unwrap();
    run_handle.await.unwrap().expect("run_loop exited cleanly");
}
