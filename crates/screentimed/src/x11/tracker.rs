use super::atoms::Atoms;
use super::common::{fetch_window_info, query_root_active_window, unwatch_window, watch_window};
use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
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
    wake_writer: Option<StdUnixStream>,
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
            wake_writer: None,
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

    fn signal_stop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(mut wake) = self.wake_writer.take() {
            let _ = wake.write_all(&[1]);
        }
    }
}

impl Drop for X11WindowTracker {
    fn drop(&mut self) {
        self.signal_stop();
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

        let (wake_reader, wake_writer) = StdUnixStream::pair().map_err(TrackerError::Io)?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let (init_tx, init_rx) = oneshot::channel();
        self.stop_tx = Some(stop_tx);
        self.wake_writer = Some(wake_writer);

        let display = self.display.clone();
        let current_window = Arc::clone(&self.current_window);

        let handle = thread::Builder::new()
            .name("screentime-x11-tracker".into())
            .spawn(move || {
                run_tracker_loop(display, tx, current_window, stop_rx, wake_reader, init_tx);
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
                .map_err(|e| {
                    TrackerError::Tracker(format!("Failed to intern atoms reply: {e}"))
                })?;

            let win_id = query_root_active_window(&conn, root, &atoms);
            Ok(win_id.and_then(|w| fetch_window_info(&conn, w, &atoms)))
        })
        .await
        .map_err(|e| TrackerError::Other(e.to_string()))?
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        self.signal_stop();
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

struct X11LoopState<'a> {
    conn: &'a RustConnection,
    root: Window,
    atoms: &'a Atoms,
    active_window_id: Option<Window>,
    active_window_info: Option<WindowInfo>,
    current_window: &'a Arc<Mutex<Option<WindowInfo>>>,
    tx: &'a mpsc::Sender<Option<WindowInfo>>,
}

impl X11LoopState<'_> {
    fn handle_property_notify(&mut self, ev: &PropertyNotifyEvent) {
        if ev.window == self.root && ev.atom == self.atoms._NET_ACTIVE_WINDOW {
            let new_active_id = query_root_active_window(self.conn, self.root, self.atoms);
            if new_active_id != self.active_window_id {
                if let Some(old_id) = self.active_window_id {
                    unwatch_window(self.conn, old_id);
                }
                self.active_window_id = new_active_id;
                let new_info = new_active_id.and_then(|w| {
                    watch_window(self.conn, w);
                    fetch_window_info(self.conn, w, self.atoms)
                });

                if new_info != self.active_window_info {
                    self.active_window_info = new_info.clone();
                    if let Ok(mut guard) = self.current_window.lock() {
                        *guard = new_info.clone();
                    }
                    let _ = self.tx.blocking_send(new_info);
                }
            }
        } else if let Some(active_id) = self.active_window_id {
            if ev.window == active_id
                && (ev.atom == self.atoms._NET_WM_NAME
                    || ev.atom == u32::from(AtomEnum::WM_NAME)
                    || ev.atom == u32::from(AtomEnum::WM_CLASS))
            {
                let updated_info = fetch_window_info(self.conn, active_id, self.atoms);
                if updated_info != self.active_window_info {
                    self.active_window_info = updated_info.clone();
                    if let Ok(mut guard) = self.current_window.lock() {
                        *guard = updated_info.clone();
                    }
                    let _ = self.tx.blocking_send(updated_info);
                }
            }
        }
    }
}

fn run_tracker_loop(
    display: Option<String>,
    tx: mpsc::Sender<Option<WindowInfo>>,
    current_window: Arc<Mutex<Option<WindowInfo>>>,
    stop_rx: watch::Receiver<bool>,
    wake_reader: StdUnixStream,
    init_tx: oneshot::Sender<Result<(), TrackerError>>,
) {
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

    let active_window_id = query_root_active_window(&conn, root, &atoms);
    let active_window_info = active_window_id.and_then(|w| {
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

    let _ = init_tx.send(Ok(()));
    info!("X11 window tracker loop initialized successfully");

    let mut state = X11LoopState {
        conn: &conn,
        root,
        atoms: &atoms,
        active_window_id,
        active_window_info,
        current_window: &current_window,
        tx: &tx,
    };

    let x11_fd = conn.stream().as_raw_fd();
    let wake_fd = wake_reader.as_raw_fd();

    while !*stop_rx.borrow() {
        loop {
            match conn.poll_for_event() {
                Ok(Some(x11rb::protocol::Event::PropertyNotify(ev))) => {
                    state.handle_property_notify(&ev);
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => {
                    debug!("X11 event stream disconnected: {e}");
                    return;
                }
            }
        }

        if *stop_rx.borrow() {
            break;
        }

        let mut fds = [
            libc::pollfd {
                fd: x11_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        if fds[1].revents != 0 || *stop_rx.borrow() {
            break;
        }

        if (fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0
            && (fds[0].revents & libc::POLLIN) == 0
        {
            break;
        }
    }

    if let Some(active_id) = state.active_window_id {
        unwatch_window(&conn, active_id);
    }

    debug!("X11 window tracker thread stopped cleanly");
}
