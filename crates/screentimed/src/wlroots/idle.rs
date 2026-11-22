use super::dispatch::{run_wayland_event_loop, OutgoingDrainer, WaylandConnectionSource};
use async_trait::async_trait;
use screentime_core::{IdleDetector, IdleState, TrackerError};
use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1::{self, ZwlrOutputPowerManagerV1},
    zwlr_output_power_v1::{self, Mode, ZwlrOutputPowerV1},
};

/// Internal Wayland dispatch state for composite idle detection.
pub struct WaylandIdleState {
    pub registry: Option<WlRegistry>,
    pub notifier: Option<ExtIdleNotifierV1>,
    pub notification_by_global: HashMap<u32, ExtIdleNotificationV1>,
    pub idled_notifications: HashSet<ObjectId>,

    pub output_power_mgr: Option<ZwlrOutputPowerManagerV1>,
    pub output_power_by_global: HashMap<u32, ZwlrOutputPowerV1>,
    pub output_modes: HashMap<ObjectId, bool>, // true = On, false = Off

    pub seat_globals: HashMap<u32, WlSeat>,
    pub output_globals: HashMap<u32, WlOutput>,

    pub idle_timeout_ms: u32,
    pub current_is_idle: bool,
    pub shared_is_idle: Arc<AtomicBool>,
    pub outgoing: VecDeque<IdleState>,
}

impl OutgoingDrainer for WaylandIdleState {
    type Event = IdleState;

    fn pop_outgoing(&mut self) -> Option<Self::Event> {
        self.outgoing.pop_front()
    }
}

/// Evaluates composite idle status from display power states and input seat notifications.
pub fn compute_idle_state(
    output_power_modes: impl IntoIterator<Item = bool>,
    total_notifications: usize,
    idled_notifications_count: usize,
) -> bool {
    let mut any_output = false;
    let mut any_output_on = false;
    for on in output_power_modes {
        any_output = true;
        if on {
            any_output_on = true;
            break;
        }
    }

    let screens_off = any_output && !any_output_on;
    let input_idle = total_notifications > 0 && idled_notifications_count == total_notifications;
    screens_off || input_idle
}

impl WaylandIdleState {
    /// Recalculates composite idle state from DPMS screen power and input timeout.
    pub fn recalculate_idle(&mut self) {
        let new_is_idle = compute_idle_state(
            self.output_modes.values().copied(),
            self.notification_by_global.len(),
            self.idled_notifications.len(),
        );

        if new_is_idle != self.current_is_idle {
            self.current_is_idle = new_is_idle;
            self.shared_is_idle.store(new_is_idle, Ordering::SeqCst);
            let state = if new_is_idle {
                IdleState::Idle
            } else {
                IdleState::Active
            };
            self.outgoing.push_back(state);
        }
    }

    /// Removes protocol objects associated with a hot-unplugged Wayland global.
    pub fn handle_global_remove(&mut self, name: u32) {
        let mut changed = false;

        if self.output_globals.remove(&name).is_some() {
            if let Some(power) = self.output_power_by_global.remove(&name) {
                self.output_modes.remove(&power.id());
                power.destroy();
            }
            changed = true;
        }

        if self.seat_globals.remove(&name).is_some() {
            if let Some(notif) = self.notification_by_global.remove(&name) {
                self.idled_notifications.remove(&notif.id());
                notif.destroy();
            }
            changed = true;
        }

        if changed {
            self.recalculate_idle();
        }
    }

    /// Cleans up all protocol objects upon shutdown.
    pub fn cleanup(&mut self) {
        for (_, notif) in self.notification_by_global.drain() {
            notif.destroy();
        }
        if let Some(notifier) = self.notifier.take() {
            notifier.destroy();
        }
        for (_, power) in self.output_power_by_global.drain() {
            power.destroy();
        }
        if let Some(mgr) = self.output_power_mgr.take() {
            mgr.destroy();
        }
        self.seat_globals.clear();
        self.output_globals.clear();
        self.output_modes.clear();
        self.idled_notifications.clear();
    }
}

impl Dispatch<WlRegistry, ()> for WaylandIdleState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "ext_idle_notifier_v1" => {
                    let notifier =
                        registry.bind::<ExtIdleNotifierV1, _, _>(name, version.min(1), qh, ());
                    for (&seat_global, seat) in &state.seat_globals {
                        let notif =
                            notifier.get_idle_notification(state.idle_timeout_ms, seat, qh, ());
                        state.notification_by_global.insert(seat_global, notif);
                    }
                    state.notifier = Some(notifier);
                }
                "zwlr_output_power_manager_v1" => {
                    let mgr = registry.bind::<ZwlrOutputPowerManagerV1, _, _>(
                        name,
                        version.min(1),
                        qh,
                        (),
                    );
                    for (&output_global, output) in &state.output_globals {
                        let power = mgr.get_output_power(output, qh, ());
                        state.output_power_by_global.insert(output_global, power);
                    }
                    state.output_power_mgr = Some(mgr);
                }
                "wl_seat" => {
                    let seat = registry.bind::<WlSeat, _, _>(name, version.min(1), qh, ());
                    if let Some(ref notifier) = state.notifier {
                        let notif =
                            notifier.get_idle_notification(state.idle_timeout_ms, &seat, qh, ());
                        state.notification_by_global.insert(name, notif);
                    }
                    state.seat_globals.insert(name, seat);
                }
                "wl_output" => {
                    let output = registry.bind::<WlOutput, _, _>(name, version.min(1), qh, ());
                    if let Some(ref mgr) = state.output_power_mgr {
                        let power = mgr.get_output_power(&output, qh, ());
                        state.output_power_by_global.insert(name, power);
                    }
                    state.output_globals.insert(name, output);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.handle_global_remove(name);
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for WaylandIdleState {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for WaylandIdleState {
    fn event(
        state: &mut Self,
        notification: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                state.idled_notifications.insert(notification.id());
                state.recalculate_idle();
            }
            ext_idle_notification_v1::Event::Resumed => {
                state.idled_notifications.remove(&notification.id());
                state.recalculate_idle();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for WaylandIdleState {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputPowerManagerV1,
        _: zwlr_output_power_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrOutputPowerV1, ()> for WaylandIdleState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputPowerV1,
        event: zwlr_output_power_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_power_v1::Event::Mode { mode } => {
                let is_on = match mode {
                    WEnum::Value(Mode::On) => true,
                    WEnum::Value(Mode::Off) => false,
                    _ => true,
                };
                state.output_modes.insert(proxy.id(), is_on);
                state.recalculate_idle();
            }
            zwlr_output_power_v1::Event::Failed => {
                debug!("zwlr_output_power_v1 mode query/change failed");
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for WaylandIdleState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wayland_client::protocol::wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, ()> for WaylandIdleState {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: wayland_client::protocol::wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn setup_idle_session(
    conn: &Connection,
    timeout_ms: u32,
    is_idle_atomic: Arc<AtomicBool>,
) -> Result<(EventQueue<WaylandIdleState>, WaylandIdleState), TrackerError> {
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let display = conn.display();

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
        idle_timeout_ms: timeout_ms,
        current_is_idle: is_idle_atomic.load(Ordering::SeqCst),
        shared_is_idle: is_idle_atomic,
        outgoing: VecDeque::new(),
    };

    state.registry = Some(display.get_registry(&qh, ()));

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| TrackerError::Tracker(format!("Initial Wayland roundtrip failed: {e}")))?;

    if state.notifier.is_none() && state.output_power_mgr.is_none() {
        return Err(TrackerError::Tracker(
            "Wayland compositor does not support ext_idle_notifier_v1 or zwlr_output_power_manager_v1".to_string(),
        ));
    }

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| TrackerError::Tracker(format!("Wayland idle setup roundtrip failed: {e}")))?;

    Ok((event_queue, state))
}

/// Generic Wayland idle detector.
///
/// Binds `ext-idle-notify-v1` to detect user inactivity while honoring Wayland
/// idle inhibitors (media playback, conferences), and `zwlr_output_power_manager_v1`
/// to immediately detect monitor screen blanking (DPMS).
pub struct WaylandIdleDetector {
    connection_source: WaylandConnectionSource,
    idle_timeout: Duration,
    is_idle: Arc<AtomicBool>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<Result<(), TrackerError>>>,
}

impl Default for WaylandIdleDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandIdleDetector {
    /// Creates an idle detector connecting to the display from the environment ($WAYLAND_DISPLAY).
    pub fn new() -> Self {
        Self {
            connection_source: WaylandConnectionSource::Env,
            idle_timeout: Duration::from_secs(180),
            is_idle: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            join_handle: None,
        }
    }

    /// Creates an idle detector from a custom Unix socket stream (for mock servers and tests).
    pub fn from_socket_stream(stream: StdUnixStream) -> Self {
        Self {
            connection_source: WaylandConnectionSource::CustomStream(Some(stream)),
            idle_timeout: Duration::from_secs(180),
            is_idle: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            join_handle: None,
        }
    }

    /// Sets the user idle timeout duration (default: 180 seconds).
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Returns the configured idle timeout duration.
    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }

    /// Returns true if the idle detector background event loop is running.
    pub fn is_running(&self) -> bool {
        self.stop_tx.is_some()
    }
}

#[async_trait]
impl IdleDetector for WaylandIdleDetector {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        let can_reconnect = self.connection_source.is_env();
        let conn = self.connection_source.connect()?;
        let timeout_ms = self.idle_timeout.as_millis().min(u32::MAX as u128) as u32;
        let is_idle_atomic = self.is_idle.clone();

        let (event_queue, state) =
            setup_idle_session(&conn, timeout_ms, Arc::clone(&is_idle_atomic))?;

        let initial_state = if state.current_is_idle {
            IdleState::Idle
        } else {
            IdleState::Active
        };
        if tx.send(initial_state).await.is_err() {
            return Ok(());
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

                    warn!("Wayland idle connection lost; attempting to reconnect...");
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
                    Ok(new_conn) => match setup_idle_session(
                        &new_conn,
                        timeout_ms,
                        Arc::clone(&is_idle_atomic),
                    ) {
                        Ok((new_queue, new_state)) => {
                            info!("Reconnected Wayland idle detector to compositor");
                            active = Some((new_conn, new_queue, new_state));
                        }
                        Err(e) => {
                            debug!("Wayland idle reconnect setup failed: {e}");
                        }
                    },
                    Err(e) => {
                        debug!("Wayland idle reconnect failed: {e}");
                    }
                }
            }
        });

        self.join_handle = Some(handle);
        Ok(())
    }

    fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
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

    fn idle_timeout(&self) -> Option<Duration> {
        Some(self.idle_timeout)
    }
}
