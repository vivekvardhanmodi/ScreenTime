use super::common::evaluate_idle_state;
use async_trait::async_trait;
use screentime_core::{IdleDetector, IdleState, TrackerError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::screensaver;
use x11rb::rust_connection::RustConnection;

/// Linux X11 User Idle Detector using pure-Rust `x11rb` and `MIT-SCREEN-SAVER`.
pub struct X11IdleDetector {
    display: Option<String>,
    idle_threshold: Duration,
    poll_interval: Duration,
    is_idle: Arc<AtomicBool>,
    stop_signal: Option<Arc<(Mutex<bool>, Condvar)>>,
    join_handle: Option<JoinHandle<()>>,
}

impl Default for X11IdleDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl X11IdleDetector {
    /// Create a new X11IdleDetector using the default `$DISPLAY`, 120s idle threshold, and 1s poll interval.
    pub fn new() -> Self {
        Self {
            display: None,
            idle_threshold: Duration::from_secs(120),
            poll_interval: Duration::from_secs(1),
            is_idle: Arc::new(AtomicBool::new(false)),
            stop_signal: None,
            join_handle: None,
        }
    }

    /// Set an explicit X11 display name (e.g. `":0"`, `":1"`, or Unix socket path).
    pub fn with_display(mut self, display: impl Into<String>) -> Self {
        self.display = Some(display.into());
        self
    }

    /// Set the duration of user inactivity after which the user is marked idle.
    pub fn with_idle_threshold(mut self, threshold: Duration) -> Self {
        self.idle_threshold = threshold;
        self
    }

    /// Set the interval at which the hardware idle time is queried.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Returns the configured display name, if any.
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref()
    }

    /// Returns the configured idle threshold.
    pub fn idle_threshold(&self) -> Duration {
        self.idle_threshold
    }

    /// Returns the configured polling interval.
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Returns true if the idle detector worker thread is actively running.
    pub fn is_running(&self) -> bool {
        self.join_handle.is_some()
    }

    fn signal_stop(&mut self) {
        if let Some(signal) = self.stop_signal.take() {
            let (lock, cvar) = &*signal;
            if let Ok(mut stopped) = lock.lock() {
                *stopped = true;
                cvar.notify_all();
            }
        }
    }

    /// Query the current user idle duration from the X server in a one-off query.
    /// If the `MIT-SCREEN-SAVER` extension is unsupported, returns `Duration::ZERO`.
    pub async fn query_idle_duration(&self) -> Result<Duration, TrackerError> {
        let display = self.display.clone();
        tokio::task::spawn_blocking(move || {
            let (conn, screen_num) = RustConnection::connect(display.as_deref())
                .map_err(|e| TrackerError::Tracker(format!("Failed to connect to X11: {e}")))?;
            let screen = &conn.setup().roots[screen_num];
            let root = screen.root;

            let has_screensaver = match screensaver::query_version(&conn, 1, 1) {
                Ok(cookie) => cookie.reply().is_ok(),
                Err(_) => false,
            };

            if !has_screensaver {
                return Ok(Duration::ZERO);
            }

            let result = match screensaver::query_info(&conn, root) {
                Ok(cookie) => match cookie.reply() {
                    Ok(reply) => Ok(Duration::from_millis(reply.ms_since_user_input as u64)),
                    Err(_) => Ok(Duration::ZERO),
                },
                Err(_) => Ok(Duration::ZERO),
            };
            result
        })
        .await
        .map_err(|e| TrackerError::Other(e.to_string()))?
    }

    /// Query the current IdleState from the X server in a one-off query.
    /// If the `MIT-SCREEN-SAVER` extension is unsupported, returns `IdleState::Active`.
    pub async fn query_idle_state(&self) -> Result<IdleState, TrackerError> {
        let display = self.display.clone();
        let threshold = self.idle_threshold;
        tokio::task::spawn_blocking(move || {
            let (conn, screen_num) = RustConnection::connect(display.as_deref())
                .map_err(|e| TrackerError::Tracker(format!("Failed to connect to X11: {e}")))?;
            let screen = &conn.setup().roots[screen_num];
            let root = screen.root;

            let has_screensaver = match screensaver::query_version(&conn, 1, 1) {
                Ok(cookie) => cookie.reply().is_ok(),
                Err(_) => false,
            };

            if !has_screensaver {
                return Ok(IdleState::Active);
            }

            let result = match screensaver::query_info(&conn, root) {
                Ok(cookie) => match cookie.reply() {
                    Ok(reply) => Ok(evaluate_idle_state(
                        reply.ms_since_user_input,
                        reply.state,
                        threshold,
                    )),
                    Err(_) => Ok(IdleState::Active),
                },
                Err(_) => Ok(IdleState::Active),
            };
            result
        })
        .await
        .map_err(|e| TrackerError::Other(e.to_string()))?
    }
}

impl Drop for X11IdleDetector {
    fn drop(&mut self) {
        self.signal_stop();
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

#[async_trait]
impl IdleDetector for X11IdleDetector {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        if self.join_handle.is_some() {
            return Ok(());
        }

        let stop_signal = Arc::new((Mutex::new(false), Condvar::new()));
        let (init_tx, init_rx) = oneshot::channel();
        self.stop_signal = Some(Arc::clone(&stop_signal));

        let display = self.display.clone();
        let idle_threshold = self.idle_threshold;
        let poll_interval = self.poll_interval;
        let is_idle = Arc::clone(&self.is_idle);

        let handle = thread::Builder::new()
            .name("screentime-x11-idle".into())
            .spawn(move || {
                run_idle_loop(
                    display,
                    idle_threshold,
                    poll_interval,
                    tx,
                    is_idle,
                    stop_signal,
                    init_tx,
                );
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
                    "X11 idle detector thread exited during initialization".to_string(),
                ))
            }
        }
    }

    fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        self.signal_stop();
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    fn idle_timeout(&self) -> Option<Duration> {
        Some(self.idle_threshold)
    }
}

fn run_idle_loop(
    display: Option<String>,
    idle_threshold: Duration,
    poll_interval: Duration,
    tx: mpsc::Sender<IdleState>,
    is_idle: Arc<AtomicBool>,
    stop_signal: Arc<(Mutex<bool>, Condvar)>,
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

    let has_screensaver = match screensaver::query_version(&conn, 1, 1) {
        Ok(cookie) => match cookie.reply() {
            Ok(v) => {
                debug!(
                    "XScreenSaver version: {}.{}",
                    v.server_major_version, v.server_minor_version
                );
                true
            }
            Err(e) => {
                warn!("XScreenSaver extension not supported: {e}. Idle detection disabled.");
                false
            }
        },
        Err(e) => {
            warn!("Failed to query XScreenSaver extension: {e}. Idle detection disabled.");
            false
        }
    };

    let mut current_state = if has_screensaver {
        match screensaver::query_info(&conn, root) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => {
                    evaluate_idle_state(reply.ms_since_user_input, reply.state, idle_threshold)
                }
                Err(e) => {
                    warn!("Failed to read initial XScreenSaver info: {e}");
                    IdleState::Active
                }
            },
            Err(e) => {
                warn!("Failed to request initial XScreenSaver info: {e}");
                IdleState::Active
            }
        }
    } else {
        IdleState::Active
    };

    is_idle.store(current_state.is_idle(), Ordering::SeqCst);

    if tx.blocking_send(current_state).is_err() {
        let _ = init_tx.send(Ok(()));
        return;
    }

    let _ = init_tx.send(Ok(()));
    info!("X11 idle detector loop initialized successfully");

    let (lock, cvar) = &*stop_signal;
    loop {
        let guard = match lock.lock() {
            Ok(g) => g,
            Err(_) => break,
        };
        if *guard {
            break;
        }

        if has_screensaver {
            let (guard, _) = match cvar.wait_timeout_while(guard, poll_interval, |s| !*s) {
                Ok(res) => res,
                Err(_) => break,
            };
            if *guard {
                break;
            }
            drop(guard);

            if let Ok(cookie) = screensaver::query_info(&conn, root) {
                if let Ok(reply) = cookie.reply() {
                    let new_state =
                        evaluate_idle_state(reply.ms_since_user_input, reply.state, idle_threshold);

                    if new_state != current_state {
                        current_state = new_state;
                        is_idle.store(new_state.is_idle(), Ordering::SeqCst);
                        if tx.blocking_send(new_state).is_err() {
                            break;
                        }
                    }
                }
            }
        } else {
            let _ = cvar.wait_while(guard, |s| !*s);
            break;
        }
    }

    debug!("X11 idle detector thread stopped cleanly");
}
