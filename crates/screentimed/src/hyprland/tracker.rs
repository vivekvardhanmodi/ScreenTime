use super::client::{parse_active_window_line, query_active_window_at, resolve_socket_dir};
use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

pub struct HyprlandWindowTracker {
    socket_dir: Option<PathBuf>,
    retry_interval: Duration,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<()>>,
}

impl Default for HyprlandWindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl HyprlandWindowTracker {
    pub fn new() -> Self {
        Self {
            socket_dir: None,
            retry_interval: Duration::from_secs(2),
            stop_tx: None,
            join_handle: None,
        }
    }

    pub fn with_socket_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_dir = Some(path.into());
        self
    }

    pub fn with_retry_interval(mut self, interval: Duration) -> Self {
        self.retry_interval = interval;
        self
    }

    pub fn socket_dir(&self) -> Option<&Path> {
        self.socket_dir.as_deref()
    }
}

#[async_trait]
impl WindowTracker for HyprlandWindowTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        let socket_dir = resolve_socket_dir(self.socket_dir.as_deref())?;
        let retry_interval = self.retry_interval;
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let handle = tokio::spawn(async move {
            let socket2_path = socket_dir.join(".socket2.sock");

            while !*stop_rx.borrow() {
                match UnixStream::connect(&socket2_path).await {
                    Ok(stream) => {
                        debug!("Connected to Hyprland socket2 stream: {:?}", socket2_path);

                        if let Ok(initial_win) = query_active_window_at(&socket_dir).await {
                            if tx.send(initial_win).await.is_err() {
                                return;
                            }
                        }

                        let reader = BufReader::new(stream);
                        let mut lines = reader.lines();

                        loop {
                            tokio::select! {
                                _ = stop_rx.changed() => {
                                    if *stop_rx.borrow() {
                                        return;
                                    }
                                }
                                res = lines.next_line() => {
                                    match res {
                                        Ok(Some(line)) => {
                                            if let Some(win_opt) = parse_active_window_line(&line) {
                                                if tx.send(win_opt).await.is_err() {
                                                    return;
                                                }
                                            }
                                        }
                                        Ok(None) => {
                                            warn!("Hyprland socket2 closed (EOF), reconnecting...");
                                            break;
                                        }
                                        Err(e) => {
                                            warn!("Error reading Hyprland socket2: {e}");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            "Cannot connect to Hyprland socket2 ({:?}): {e}",
                            socket2_path
                        );
                    }
                }

                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            return;
                        }
                    }
                    _ = tokio::time::sleep(retry_interval) => {}
                }
            }
        });

        self.join_handle = Some(handle);
        Ok(())
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        let socket_dir = resolve_socket_dir(self.socket_dir.as_deref())?;
        query_active_window_at(&socket_dir).await
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
}
