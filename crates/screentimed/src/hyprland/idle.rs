use super::client::{
    parse_dpms_event_line, query_monitors_at, query_monitors_map_at, resolve_socket_dir,
};
use async_trait::async_trait;
use screentime_core::{IdleDetector, IdleState, TrackerError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

fn compute_idle_from_monitors(monitor_states: &HashMap<String, bool>) -> Option<IdleState> {
    if monitor_states.is_empty() {
        return None;
    }
    let any_on = monitor_states.values().any(|&on| on);
    Some(if any_on {
        IdleState::Active
    } else {
        IdleState::Idle
    })
}

pub struct HyprlandIdleDetector {
    socket_dir: Option<PathBuf>,
    poll_interval: Duration,
    retry_interval: Duration,
    idle_timeout: Option<Duration>,
    is_idle: Arc<AtomicBool>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<()>>,
}

impl Default for HyprlandIdleDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl HyprlandIdleDetector {
    pub fn new() -> Self {
        Self {
            socket_dir: None,
            poll_interval: Duration::from_secs(2),
            retry_interval: Duration::from_secs(2),
            idle_timeout: None,
            is_idle: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            join_handle: None,
        }
    }

    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    pub fn idle_timeout(&self) -> Option<Duration> {
        self.idle_timeout
    }

    pub fn with_socket_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_dir = Some(path.into());
        self
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn with_retry_interval(mut self, interval: Duration) -> Self {
        self.retry_interval = interval;
        self
    }

    pub fn socket_dir(&self) -> Option<&Path> {
        self.socket_dir.as_deref()
    }

    pub async fn query_monitors(&self) -> Result<Option<IdleState>, TrackerError> {
        let socket_dir = resolve_socket_dir(self.socket_dir.as_deref())?;
        query_monitors_at(&socket_dir).await
    }
}

#[async_trait]
impl IdleDetector for HyprlandIdleDetector {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        let socket_dir = resolve_socket_dir(self.socket_dir.as_deref())?;
        let is_idle_atomic = self.is_idle.clone();
        let poll_interval = self.poll_interval;
        let retry_interval = self.retry_interval;
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let handle = tokio::spawn(async move {
            let socket2_path = socket_dir.join(".socket2.sock");
            let mut poll_ticker = tokio::time::interval(poll_interval);
            poll_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut monitor_states: HashMap<String, bool> = HashMap::new();

            if let Ok(Some(initial_map)) = query_monitors_map_at(&socket_dir).await {
                monitor_states = initial_map;
                if let Some(initial_state) = compute_idle_from_monitors(&monitor_states) {
                    is_idle_atomic.store(initial_state.is_idle(), Ordering::SeqCst);
                    if tx.send(initial_state).await.is_err() {
                        return;
                    }
                }
            }

            poll_ticker.tick().await;

            while !*stop_rx.borrow() {
                match UnixStream::connect(&socket2_path).await {
                    Ok(stream) => {
                        debug!("IdleDetector connected to socket2: {:?}", socket2_path);
                        let reader = BufReader::new(stream);
                        let mut lines = reader.lines();

                        loop {
                            tokio::select! {
                                _ = stop_rx.changed() => {
                                    if *stop_rx.borrow() {
                                        return;
                                    }
                                }
                                _ = poll_ticker.tick() => {
                                    if let Ok(Some(polled)) = query_monitors_map_at(&socket_dir).await {
                                        monitor_states = polled;
                                        if let Some(state) = compute_idle_from_monitors(&monitor_states) {
                                            let was_idle = is_idle_atomic.swap(state.is_idle(), Ordering::SeqCst);
                                            if was_idle != state.is_idle() && tx.send(state).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                                res = lines.next_line() => {
                                    match res {
                                        Ok(Some(line)) => {
                                            if let Some((mon_opt, is_on)) = parse_dpms_event_line(&line) {
                                                let next_state = match mon_opt {
                                                    Some(mon) => {
                                                        monitor_states.insert(mon, is_on);
                                                        compute_idle_from_monitors(&monitor_states)
                                                    }
                                                    None => {
                                                        for v in monitor_states.values_mut() {
                                                            *v = is_on;
                                                        }
                                                        Some(if is_on {
                                                            IdleState::Active
                                                        } else {
                                                            IdleState::Idle
                                                        })
                                                    }
                                                };

                                                if let Some(state) = next_state {
                                                    let was_idle = is_idle_atomic.swap(state.is_idle(), Ordering::SeqCst);
                                                    if was_idle != state.is_idle() && tx.send(state).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                        Ok(None) => {
                                            warn!("IdleDetector socket2 EOF received");
                                            break;
                                        }
                                        Err(e) => {
                                            warn!("IdleDetector error reading socket2: {e}");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!("IdleDetector cannot connect to socket2: {e}");
                        tokio::select! {
                            _ = stop_rx.changed() => {
                                if *stop_rx.borrow() {
                                    return;
                                }
                            }
                            _ = poll_ticker.tick() => {
                                if let Ok(Some(polled)) = query_monitors_map_at(&socket_dir).await {
                                    monitor_states = polled;
                                    if let Some(state) = compute_idle_from_monitors(&monitor_states) {
                                        let was_idle = is_idle_atomic.swap(state.is_idle(), Ordering::SeqCst);
                                        if was_idle != state.is_idle() && tx.send(state).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            _ = tokio::time::sleep(retry_interval) => {}
                        }
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
            let _ = handle.await;
        }
        Ok(())
    }

    fn idle_timeout(&self) -> Option<Duration> {
        self.idle_timeout
    }
}
