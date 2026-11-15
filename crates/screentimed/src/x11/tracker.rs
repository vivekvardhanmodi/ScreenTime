use super::atoms::Atoms;
use super::common::{fetch_window_info, query_root_active_window, watch_window};
use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, info};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, PropertyNotifyEvent, Window,
};
use x11rb::rust_connection::RustConnection;

/// Linux X11 Active Window Tracker using pure-Rust `x11rb`.
pub struct X11WindowTracker {
    display: Option<String>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<()>>,
    current_window: Arc<Mutex<Option<WindowInfo>>>,
}

impl Default for X11WindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl X11WindowTracker {
    /// Create a new X11WindowTracker using the default `$DISPLAY`.
    pub fn new() -> Self {
        Self {
            display: None,
            stop_tx: None,
            join_handle: None,
            current_window: Arc::new(Mutex::new(None)),
        }
    }

    /// Set an explicit X11 display name (e.g. `":0"`, `":1"`, or Unix socket path).
    pub fn with_display(mut self, display: impl Into<String>) -> Self {
        self.display = Some(display.into());
        self
    }

    /// Returns the configured display name, if any.
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref()
    }

    /// Returns true if the tracker worker thread is actively running.
    pub fn is_running(&self) -> bool {
        self.join_handle.is_some()
    }
}

impl Drop for X11WindowTracker {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

#[async_trait]
impl WindowTracker for X11WindowTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        if self.join_handle.is_some() {
            return Ok(());
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        let (init_tx, init_rx) = oneshot::channel();
        self.stop_tx = Some(stop_tx);

        let display = self.display.clone();
        let current_window = Arc::clone(&self.current_window);

        let handle = thread::Builder::new()
            .name("screentime-x11-tracker".into())
            .spawn(move || {
                run_tracker_loop(display, tx, current_window, stop_rx, init_tx);
            })
            .map_err(TrackerError::Io)?;

        self.join_handle = Some(handle);

        match init_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                let _ = self.stop().await;
                Err(e)
            }
            Err(_) => {
                let _ = self.stop().await;
                Err(TrackerError::Tracker(
                    "X11 tracker thread exited during initialization".to_string(),
                ))
            }
        }
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        if self.join_handle.is_some() {
            let guard = self
                .current_window
                .lock()
                .map_err(|e| TrackerError::Other(e.to_string()))?;
            return Ok(guard.clone());
        }

        let display = self.display.clone();
        tokio::task::spawn_blocking(move || {
            let (conn, screen_num) = RustConnection::connect(display.as_deref())
                .map_err(|e| TrackerError::Tracker(format!("Failed to connect to X11: {e}")))?;
            let screen = &conn.setup().roots[screen_num];
            let root = screen.root;
            let atoms = Atoms::new(&conn)
                .map_err(|e| TrackerError::Tracker(format!("Failed to intern atoms: {e}")))?
                .reply()
                .map_err(|e| TrackerError::Tracker(format!("Failed to intern atoms reply: {e}")))?;

            let win_id = query_root_active_window(&conn, root, &atoms);
            Ok(win_id.and_then(|w| fetch_window_info(&conn, w, &atoms)))
        })
        .await
        .map_err(|e| TrackerError::Other(e.to_string()))?
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

fn run_tracker_loop(
    display: Option<String>,
    tx: mpsc::Sender<Option<WindowInfo>>,
    current_window: Arc<Mutex<Option<WindowInfo>>>,
    stop_rx: watch::Receiver<bool>,
    init_tx: oneshot::Sender<Result<(), TrackerError>>,
) {
    // 1. Establish pure-Rust connection to X11
    let (conn, screen_num) = match RustConnection::connect(display.as_deref()) {
        Ok(res) => res,
        Err(e) => {
            let _ = init_tx.send(Err(TrackerError::Tracker(format!(
                "Failed to connect to X11 display: {e}"
            ))));
            return;
        }
    };

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // 2. Batch intern required atoms
    let atoms = match Atoms::new(&conn) {
        Ok(cookie) => match cookie.reply() {
            Ok(atoms) => atoms,
            Err(e) => {
                let _ = init_tx.send(Err(TrackerError::Tracker(format!(
                    "Failed to resolve X11 atoms: {e}"
                ))));
                return;
            }
        },
        Err(e) => {
            let _ = init_tx.send(Err(TrackerError::Tracker(format!(
                "Failed to request X11 atoms: {e}"
            ))));
            return;
        }
    };

    // 3. Subscribe to PropertyNotify events on root window
    let root_mask = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
    if let Err(e) = conn.change_window_attributes(root, &root_mask) {
        let _ = init_tx.send(Err(TrackerError::Tracker(format!(
            "Failed to subscribe to root window property changes: {e}"
        ))));
        return;
    }
    if let Err(e) = conn.flush() {
        let _ = init_tx.send(Err(TrackerError::Tracker(format!(
            "Failed to flush X11 connection: {e}"
        ))));
        return;
    }

    // 4. Bootstrap initial active window
    let mut active_window_id = query_root_active_window(&conn, root, &atoms);
    let mut active_window_info = active_window_id.and_then(|w| {
        watch_window(&conn, w);
        fetch_window_info(&conn, w, &atoms)
    });

    if let Ok(mut guard) = current_window.lock() {
        *guard = active_window_info.clone();
    }

    if tx.blocking_send(active_window_info.clone()).is_err() {
        let _ = init_tx.send(Ok(()));
        return;
    }

    // Notify caller that initialization succeeded
    let _ = init_tx.send(Ok(()));
    info!("X11 window tracker loop initialized successfully");

    // 5. Main event loop
    while !*stop_rx.borrow() {
        let mut had_events = false;

        while let Ok(Some(event)) = conn.poll_for_event() {
            had_events = true;
            if let x11rb::protocol::Event::PropertyNotify(ev) = event {
                handle_property_notify(
                    &conn,
                    root,
                    &atoms,
                    &ev,
                    &mut active_window_id,
                    &mut active_window_info,
                    &current_window,
                    &tx,
                );
            }
        }

        if *stop_rx.borrow() {
            break;
        }

        if !had_events {
            thread::sleep(Duration::from_millis(30));
        }
    }

    debug!("X11 window tracker thread stopped cleanly");
}

#[allow(clippy::too_many_arguments)]
fn handle_property_notify(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    ev: &PropertyNotifyEvent,
    active_window_id: &mut Option<Window>,
    active_window_info: &mut Option<WindowInfo>,
    current_window: &Arc<Mutex<Option<WindowInfo>>>,
    tx: &mpsc::Sender<Option<WindowInfo>>,
) {
    // Case 1: _NET_ACTIVE_WINDOW changed on root window
    if ev.window == root && ev.atom == atoms._NET_ACTIVE_WINDOW {
        let new_active_id = query_root_active_window(conn, root, atoms);
        if new_active_id != *active_window_id {
            *active_window_id = new_active_id;
            let new_info = new_active_id.and_then(|w| {
                watch_window(conn, w);
                fetch_window_info(conn, w, atoms)
            });

            if new_info != *active_window_info {
                *active_window_info = new_info.clone();
                if let Ok(mut guard) = current_window.lock() {
                    *guard = new_info.clone();
                }
                let _ = tx.blocking_send(new_info);
            }
        }
    }
    // Case 2: Title or class changed on currently active window
    else if let Some(active_id) = *active_window_id {
        if ev.window == active_id
            && (ev.atom == atoms._NET_WM_NAME
                || ev.atom == u32::from(AtomEnum::WM_NAME)
                || ev.atom == u32::from(AtomEnum::WM_CLASS))
        {
            let updated_info = fetch_window_info(conn, active_id, atoms);
            if updated_info != *active_window_info {
                *active_window_info = updated_info.clone();
                if let Ok(mut guard) = current_window.lock() {
                    *guard = updated_info.clone();
                }
                let _ = tx.blocking_send(updated_info);
            }
        }
    }
}
