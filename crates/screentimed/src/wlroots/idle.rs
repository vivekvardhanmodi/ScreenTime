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
use tracing::debug;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
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
    pub notifications: Vec<ExtIdleNotificationV1>,
    pub idled_notifications: HashSet<ObjectId>,

    pub output_power_mgr: Option<ZwlrOutputPowerManagerV1>,
    pub output_powers: Vec<ZwlrOutputPowerV1>,
    pub output_modes: HashMap<ObjectId, bool>, // true = On, false = Off

    pub seats: Vec<WlSeat>,
    pub outputs: Vec<WlOutput>,

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
            self.notifications.len(),
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

    /// Cleans up all protocol objects upon shutdown.
    pub fn cleanup(&mut self) {
        for notif in self.notifications.drain(..) {
            notif.destroy();
        }
        if let Some(notifier) = self.notifier.take() {
            notifier.destroy();
        }
        for power in self.output_powers.drain(..) {
            power.destroy();
        }
        if let Some(mgr) = self.output_power_mgr.take() {
            mgr.destroy();
        }
        self.seats.clear();
        self.outputs.clear();
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
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "ext_idle_notifier_v1" => {
                    let notifier =
                        registry.bind::<ExtIdleNotifierV1, _, _>(name, version.min(1), qh, ());
                    for seat in &state.seats {
                        let notif =
                            notifier.get_idle_notification(state.idle_timeout_ms, seat, qh, ());
                        state.notifications.push(notif);
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
                    for output in &state.outputs {
                        let power = mgr.get_output_power(output, qh, ());
                        state.output_powers.push(power);
                    }
                    state.output_power_mgr = Some(mgr);
                }
                "wl_seat" => {
                    let seat = registry.bind::<WlSeat, _, _>(name, version.min(1), qh, ());
                    if let Some(ref notifier) = state.notifier {
                        let notif =
                            notifier.get_idle_notification(state.idle_timeout_ms, &seat, qh, ());
                        state.notifications.push(notif);
                    }
                    state.seats.push(seat);
                }
                "wl_output" => {
                    let output = registry.bind::<WlOutput, _, _>(name, version.min(1), qh, ());
                    if let Some(ref mgr) = state.output_power_mgr {
                        let power = mgr.get_output_power(&output, qh, ());
                        state.output_powers.push(power);
                    }
                    state.outputs.push(output);
                }
                _ => {}
            }
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
        let conn = self.connection_source.connect()?;
        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();
        let display = conn.display();

        let timeout_ms = self.idle_timeout.as_millis().min(u32::MAX as u128) as u32;
        let is_idle_atomic = self.is_idle.clone();

        let mut state = WaylandIdleState {
            registry: None,
            notifier: None,
            notifications: Vec::new(),
            idled_notifications: HashSet::new(),
            output_power_mgr: None,
            output_powers: Vec::new(),
            output_modes: HashMap::new(),
            seats: Vec::new(),
            outputs: Vec::new(),
            idle_timeout_ms: timeout_ms,
            current_is_idle: is_idle_atomic.load(Ordering::SeqCst),
            shared_is_idle: is_idle_atomic.clone(),
            outgoing: VecDeque::new(),
        };

        state.registry = Some(display.get_registry(&qh, ()));

        // Initial roundtrip to discover globals
        event_queue
            .roundtrip(&mut state)
            .map_err(|e| TrackerError::Tracker(format!("Initial Wayland roundtrip failed: {e}")))?;

        if state.notifier.is_none() && state.output_power_mgr.is_none() {
            return Err(TrackerError::Tracker(
                "Wayland compositor does not support ext_idle_notifier_v1 or zwlr_output_power_manager_v1".to_string(),
            ));
        }

        // Second roundtrip to process initial notifications / power modes
        event_queue.roundtrip(&mut state).map_err(|e| {
            TrackerError::Tracker(format!("Wayland idle setup roundtrip failed: {e}"))
        })?;

        // Send initial idle state
        let initial_idle = state.current_is_idle;
        let initial_state = if initial_idle {
            IdleState::Idle
        } else {
            IdleState::Active
        };
        if tx.send(initial_state).await.is_err() {
            return Ok(());
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let handle = tokio::spawn(async move {
            let res = run_wayland_event_loop(conn.clone(), event_queue, state, tx, stop_rx).await;

            if let Ok(mut final_state) = res {
                final_state.cleanup();
                let _ = conn.flush();
            }

            Ok(())
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
}
