use super::dispatch::{run_wayland_event_loop, OutgoingDrainer, WaylandConnectionSource};
use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use wayland_client::{
    protocol::wl_registry::{self, WlRegistry},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, State, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// Buffered state for an individual toplevel window.
///
/// Updates are latched: changes to title, app_id, or activated state
/// are buffered in pending fields until the compositor emits the atomic `done` event.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ToplevelData {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub is_active: bool,

    pub pending_app_id: Option<String>,
    pub pending_title: Option<String>,
    pub pending_is_active: bool,
}

impl ToplevelData {
    /// Commit pending attributes to the active state on `done`.
    pub fn commit(&mut self) {
        if let Some(app_id) = self.pending_app_id.take() {
            self.app_id = Some(app_id);
        }
        if let Some(title) = self.pending_title.take() {
            self.title = Some(title);
        }
        self.is_active = self.pending_is_active;
    }
}

/// Parses the Wayland state byte array and returns true if State::Activated is present.
pub fn parse_state_activated(state_bytes: &[u8]) -> bool {
    state_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_ne_bytes(*c))
        .any(|val| val == State::Activated as u32)
}

/// Internal Wayland dispatch state for wlroots foreign toplevel tracking.
pub struct WlrForeignToplevelState {
    pub registry: Option<WlRegistry>,
    pub manager: Option<ZwlrForeignToplevelManagerV1>,
    pub windows: HashMap<ZwlrForeignToplevelHandleV1, ToplevelData>,
    pub current_active: Option<WindowInfo>,
    pub shared_active: Arc<RwLock<Option<WindowInfo>>>,
    pub outgoing: VecDeque<Option<WindowInfo>>,
}

impl OutgoingDrainer for WlrForeignToplevelState {
    type Event = Option<WindowInfo>;

    fn pop_outgoing(&mut self) -> Option<Self::Event> {
        self.outgoing.pop_front()
    }
}

/// Evaluates the current active window from an iterator of toplevel data.
pub fn compute_active_window<'a>(
    windows: impl IntoIterator<Item = &'a ToplevelData>,
) -> Option<WindowInfo> {
    let active_win = windows.into_iter().find(|w| w.is_active)?;
    let app_class = active_win.app_id.as_deref().unwrap_or("").trim();
    if app_class.is_empty() {
        return None;
    }
    let title = active_win.title.as_deref().unwrap_or("").trim();
    Some(WindowInfo::new(app_class, title))
}

impl WlrForeignToplevelState {
    pub fn recalculate_active(&mut self) {
        let new_info = compute_active_window(self.windows.values());

        if new_info != self.current_active {
            self.current_active = new_info.clone();
            if let Ok(mut lock) = self.shared_active.write() {
                *lock = new_info.clone();
            }
            self.outgoing.push_back(new_info);
        }
    }

    pub fn cleanup(&mut self) {
        for (handle, _) in self.windows.drain() {
            handle.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.stop();
        }
    }
}

impl Dispatch<WlRegistry, ()> for WlrForeignToplevelState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "zwlr_foreign_toplevel_manager_v1" {
                let bind_version = version.min(3);
                state.manager = Some(registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(
                    name,
                    bind_version,
                    qh,
                    (),
                ));
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WlrForeignToplevelState {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.windows.insert(toplevel, ToplevelData::default());
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                debug!("zwlr_foreign_toplevel_manager_v1 finished by compositor");
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(WlrForeignToplevelState, ZwlrForeignToplevelManagerV1, [
        0 => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WlrForeignToplevelState {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(win) = state.windows.get_mut(handle) {
                    win.pending_title = Some(title);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(win) = state.windows.get_mut(handle) {
                    win.pending_app_id = Some(app_id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: state_bytes } => {
                let is_active = parse_state_activated(&state_bytes);
                if let Some(win) = state.windows.get_mut(handle) {
                    win.pending_is_active = is_active;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(win) = state.windows.get_mut(handle) {
                    win.commit();
                }
                state.recalculate_active();
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.windows.remove(handle);
                handle.destroy();
                state.recalculate_active();
            }
            _ => {}
        }
    }
}

fn setup_toplevel_session(
    conn: &Connection,
    shared_active: Arc<RwLock<Option<WindowInfo>>>,
) -> Result<(EventQueue<WlrForeignToplevelState>, WlrForeignToplevelState), TrackerError> {
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let display = conn.display();

    let mut state = WlrForeignToplevelState {
        registry: None,
        manager: None,
        windows: HashMap::new(),
        current_active: None,
        shared_active,
        outgoing: VecDeque::new(),
    };

    state.registry = Some(display.get_registry(&qh, ()));

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| TrackerError::Tracker(format!("Initial Wayland roundtrip failed: {e}")))?;

    if state.manager.is_none() {
        return Err(TrackerError::Tracker(
            "Wayland compositor does not support zwlr_foreign_toplevel_manager_v1".to_string(),
        ));
    }

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| TrackerError::Tracker(format!("Wayland toplevel roundtrip failed: {e}")))?;

    Ok((event_queue, state))
}

/// Generic wlroots Wayland window tracker.
///
/// Binds `zwlr_foreign_toplevel_manager_v1` to track active windows across
/// Sway, River, Wayfire, Labwc, and other wlroots-based compositors.
pub struct WlrForeignToplevelTracker {
    connection_source: WaylandConnectionSource,
    active_window: Arc<RwLock<Option<WindowInfo>>>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<Result<(), TrackerError>>>,
}

impl Default for WlrForeignToplevelTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl WlrForeignToplevelTracker {
    /// Creates a tracker connecting to the display defined by the environment ($WAYLAND_DISPLAY).
    pub fn new() -> Self {
        Self {
            connection_source: WaylandConnectionSource::Env,
            active_window: Arc::new(RwLock::new(None)),
            stop_tx: None,
            join_handle: None,
        }
    }

    /// Creates a tracker connecting to a custom Unix socket stream (for mock servers and tests).
    pub fn from_socket_stream(stream: StdUnixStream) -> Self {
        Self {
            connection_source: WaylandConnectionSource::CustomStream(Some(stream)),
            active_window: Arc::new(RwLock::new(None)),
            stop_tx: None,
            join_handle: None,
        }
    }

    /// Returns true if the tracker background event loop is running.
    pub fn is_running(&self) -> bool {
        self.stop_tx.is_some()
    }
}

#[async_trait]
impl WindowTracker for WlrForeignToplevelTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        let can_reconnect = self.connection_source.is_env();
        let conn = self.connection_source.connect()?;
        let shared_active = self.active_window.clone();

        let (event_queue, mut state) = setup_toplevel_session(&conn, Arc::clone(&shared_active))?;

        while let Some(evt) = state.outgoing.pop_front() {
            if tx.send(evt).await.is_err() {
                return Ok(());
            }
        }

        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let handle = tokio::spawn(async move {
            let mut active = Some((conn, event_queue, state));

            loop {
                if let Some((conn, event_queue, state)) = active.take() {
                    let res = run_wayland_event_loop(
                        conn.clone(),
                        event_queue,
                        state,
                        tx.clone(),
                        stop_rx.clone(),
                    )
                    .await;

                    if let Ok(mut final_state) = res {
                        final_state.cleanup();
                        let _ = conn.flush();
                        return Ok(());
                    }

                    if !can_reconnect || *stop_rx.borrow() || tx.is_closed() {
                        return Ok(());
                    }

                    warn!("Wayland toplevel connection lost; attempting to reconnect...");
                }

                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            return Ok(());
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                }

                match Connection::connect_to_env() {
                    Ok(new_conn) => match setup_toplevel_session(
                        &new_conn,
                        Arc::clone(&shared_active),
                    ) {
                        Ok((new_queue, new_state)) => {
                            info!("Reconnected wlroots foreign toplevel tracker to compositor");
                            active = Some((new_conn, new_queue, new_state));
                        }
                        Err(e) => {
                            debug!("Wayland toplevel reconnect setup failed: {e}");
                        }
                    },
                    Err(e) => {
                        debug!("Wayland toplevel reconnect failed: {e}");
                    }
                }
            }
        });

        self.join_handle = Some(handle);
        Ok(())
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        if self.stop_tx.is_some() {
            if let Ok(lock) = self.active_window.read() {
                return Ok(lock.clone());
            }
        }

        let conn = Connection::connect_to_env().map_err(|e| {
            TrackerError::Tracker(format!("Failed to connect to Wayland display: {e}"))
        })?;
        let shared_active = Arc::new(RwLock::new(None));
        let (_, mut state) = setup_toplevel_session(&conn, shared_active)?;

        let result = state.current_active.clone();
        state.cleanup();
        let _ = conn.flush();

        Ok(result)
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(handle) = self.join_handle.take() {
            match handle.await {
                Ok(res) => res?,
                Err(e) => return Err(TrackerError::Other(e.to_string())),
            }
        }
        Ok(())
    }
}
