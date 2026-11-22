use async_trait::async_trait;
use screentime_core::{IdleDetector, IdleState, TrackerError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::debug;

use super::events::PlatformEvent;
use super::math::{calculate_idle_time, calculate_poll_interval, evaluate_idle_transition};

pub type MockTickFn = Arc<dyn Fn() -> (u32, u32) + Send + Sync>;
type IdleDurationFn = Box<dyn Fn() -> Duration + Send + 'static>;

struct IdleLoopConfig {
    idle_threshold: Duration,
    resume_threshold: Duration,
    is_idle: Arc<AtomicBool>,
    tx: mpsc::Sender<IdleState>,
    sleep_tx: Option<mpsc::Sender<crate::sleep::SleepEvent>>,
}

fn spawn_idle_loop(
    mut event_rx: Option<mpsc::UnboundedReceiver<PlatformEvent>>,
    idle_duration_fn: IdleDurationFn,
    mut stop_rx: watch::Receiver<bool>,
    config: IdleLoopConfig,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut current_state = IdleState::Active;
        let mut next_poll = calculate_poll_interval(false, Duration::ZERO, config.idle_threshold);

        loop {
            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
                event_opt = async {
                    if let Some(ref mut rx) = event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match event_opt {
                        Some(PlatformEvent::DisplayOff) | Some(PlatformEvent::SessionLocked) => {
                            debug!("DisplayOff or SessionLocked received -> Idle");
                            if current_state != IdleState::Idle {
                                current_state = IdleState::Idle;
                                config.is_idle.store(true, Ordering::SeqCst);
                                if config.tx.send(IdleState::Idle).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(PlatformEvent::DisplayOn) | Some(PlatformEvent::SessionUnlocked) => {
                            debug!("DisplayOn or SessionUnlocked received -> Active");
                            if current_state != IdleState::Active {
                                current_state = IdleState::Active;
                                config.is_idle.store(false, Ordering::SeqCst);
                                if config.tx.send(IdleState::Active).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(PlatformEvent::Suspend) => {
                            debug!("Suspend received -> notifying sleep");
                            if let Some(ref stx) = config.sleep_tx {
                                let _ = stx.send(crate::sleep::SleepEvent::Sleep).await;
                            }
                            if current_state != IdleState::Idle {
                                current_state = IdleState::Idle;
                                config.is_idle.store(true, Ordering::SeqCst);
                                if config.tx.send(IdleState::Idle).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(PlatformEvent::Resume) => {
                            debug!("Resume received -> notifying wake");
                            if let Some(ref stx) = config.sleep_tx {
                                let _ = stx.send(crate::sleep::SleepEvent::Wake).await;
                            }
                            if current_state != IdleState::Active {
                                current_state = IdleState::Active;
                                config.is_idle.store(false, Ordering::SeqCst);
                                if config.tx.send(IdleState::Active).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(_) => {}
                        None => {
                            event_rx = None;
                        }
                    }
                }
                _ = tokio::time::sleep(next_poll) => {
                    let idle_duration = idle_duration_fn();

                    let new_state = evaluate_idle_transition(
                        current_state,
                        idle_duration,
                        config.idle_threshold,
                        config.resume_threshold,
                    );

                    if new_state != current_state {
                        debug!("Idle state transition: {:?} -> {:?}", current_state, new_state);
                        current_state = new_state;
                        config.is_idle.store(new_state.is_idle(), Ordering::SeqCst);
                        if config.tx.send(new_state).await.is_err() {
                            break;
                        }
                    }

                    next_poll = calculate_poll_interval(
                        current_state.is_idle(),
                        idle_duration,
                        config.idle_threshold,
                    );
                }
            }
        }
    })
}

pub struct WindowsIdleDetector {
    idle_threshold: Duration,
    resume_threshold: Duration,
    is_idle: Arc<AtomicBool>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<()>>,
    mock_event_rx: Option<mpsc::UnboundedReceiver<PlatformEvent>>,
    mock_tick_fn: Option<MockTickFn>,
    sleep_tx: Option<mpsc::Sender<crate::sleep::SleepEvent>>,
    #[cfg(target_os = "windows")]
    listener_handle: Option<super::listener::Win32ListenerHandle>,
}

impl Default for WindowsIdleDetector {
    fn default() -> Self {
        Self::new(Duration::from_secs(90))
    }
}

impl WindowsIdleDetector {
    pub fn new(idle_threshold: Duration) -> Self {
        Self {
            idle_threshold,
            resume_threshold: Duration::from_millis(1500),
            is_idle: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            join_handle: None,
            mock_event_rx: None,
            mock_tick_fn: None,
            sleep_tx: None,
            #[cfg(target_os = "windows")]
            listener_handle: None,
        }
    }

    pub fn with_sleep_sender(mut self, tx: mpsc::Sender<crate::sleep::SleepEvent>) -> Self {
        self.sleep_tx = Some(tx);
        self
    }

    pub fn with_idle_threshold(mut self, threshold: Duration) -> Self {
        self.idle_threshold = threshold;
        self
    }

    pub fn with_resume_threshold(mut self, threshold: Duration) -> Self {
        self.resume_threshold = threshold;
        self
    }

    /// Injects a mock event stream for testing display off/on and session lock/unlock events.
    pub fn with_mock_receiver(mut self, rx: mpsc::UnboundedReceiver<PlatformEvent>) -> Self {
        self.mock_event_rx = Some(rx);
        self
    }

    /// Injects a mock tick provider `(current_ticks, last_input_ticks)` for testing rollover math without Windows OS.
    pub fn with_mock_tick_fn(mut self, tick_fn: MockTickFn) -> Self {
        self.mock_tick_fn = Some(tick_fn);
        self
    }

    /// Queries the current idle time duration using hardware tick counts with 32-bit rollover protection.
    pub fn query_idle_time(&self) -> Duration {
        if let Some(ref tick_fn) = self.mock_tick_fn {
            let (current, last_input) = tick_fn();
            return calculate_idle_time(current, last_input);
        }

        #[cfg(target_os = "windows")]
        {
            match super::win32_utils::get_idle_time_millis() {
                Ok(ms) => Duration::from_millis(ms as u64),
                Err(_) => Duration::ZERO,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Duration::ZERO
        }
    }
}

#[async_trait]
impl IdleDetector for WindowsIdleDetector {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let config = IdleLoopConfig {
            idle_threshold: self.idle_threshold,
            resume_threshold: self.resume_threshold,
            is_idle: self.is_idle.clone(),
            tx,
            sleep_tx: self.sleep_tx.clone(),
        };

        if self.mock_event_rx.is_some() || self.mock_tick_fn.is_some() {
            let mock_rx = self.mock_event_rx.take();
            let mock_tick_fn = self.mock_tick_fn.clone();
            let idle_duration_fn: IdleDurationFn = Box::new(move || {
                if let Some(ref tick_fn) = mock_tick_fn {
                    let (current, last_input) = tick_fn();
                    calculate_idle_time(current, last_input)
                } else {
                    Duration::ZERO
                }
            });

            self.join_handle = Some(spawn_idle_loop(mock_rx, idle_duration_fn, stop_rx, config));
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let listener_handle = super::listener::spawn_win32_listener(event_tx);
            self.listener_handle = Some(listener_handle);

            let idle_duration_fn: IdleDurationFn = Box::new(|| {
                match super::win32_utils::get_idle_time_millis() {
                    Ok(ms) => Duration::from_millis(ms as u64),
                    Err(_) => Duration::ZERO,
                }
            });

            self.join_handle = Some(spawn_idle_loop(
                Some(event_rx),
                idle_duration_fn,
                stop_rx,
                config,
            ));
            Ok(())
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(TrackerError::Tracker(
                "WindowsIdleDetector is only supported on Windows unless configured with mock inputs"
                    .into(),
            ))
        }
    }

    fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }

        #[cfg(target_os = "windows")]
        if let Some(handle) = self.listener_handle.take() {
            handle.stop();
        }

        if let Some(handle) = self.join_handle.take() {
            let _ = handle.await;
        }

        Ok(())
    }

    fn idle_timeout(&self) -> Option<Duration> {
        Some(self.idle_threshold)
    }
}
