use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, RwLock};
use tokio::task::JoinHandle;
#[cfg(target_os = "windows")]
use tracing::debug;

use super::events::PlatformEvent;

pub struct WindowsTracker {
    current_window: Arc<RwLock<Option<WindowInfo>>>,
    stop_tx: Option<watch::Sender<bool>>,
    join_handle: Option<JoinHandle<()>>,
    mock_event_rx: Option<mpsc::UnboundedReceiver<PlatformEvent>>,
    #[cfg(target_os = "windows")]
    listener_handle: Option<super::listener::Win32ListenerHandle>,
}

impl Default for WindowsTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsTracker {
    pub fn new() -> Self {
        Self {
            current_window: Arc::new(RwLock::new(None)),
            stop_tx: None,
            join_handle: None,
            mock_event_rx: None,
            #[cfg(target_os = "windows")]
            listener_handle: None,
        }
    }

    /// Injects a mock event stream for testing on Linux or Windows without live Win32 hooks.
    pub fn with_mock_receiver(mut self, rx: mpsc::UnboundedReceiver<PlatformEvent>) -> Self {
        self.mock_event_rx = Some(rx);
        self
    }
}

#[async_trait]
impl WindowTracker for WindowsTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);
        let current_window = self.current_window.clone();

        if let Some(mut mock_rx) = self.mock_event_rx.take() {
            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                break;
                            }
                        }
                        event = mock_rx.recv() => {
                            match event {
                                Some(PlatformEvent::WindowChanged(win_opt)) => {
                                    *current_window.write().await = win_opt.clone();
                                    if tx.send(win_opt).await.is_err() {
                                        break;
                                    }
                                }
                                Some(_) => {
                                    // Other events handled by IdleDetector
                                }
                                None => break,
                            }
                        }
                    }
                }
            });
            self.join_handle = Some(handle);
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            let (event_tx, mut event_rx) = mpsc::unbounded_channel();
            let listener_handle = super::listener::spawn_win32_listener(event_tx);
            self.listener_handle = Some(listener_handle);

            // Seed initial window if available
            if let Ok(Some(initial)) = self.query_active_window().await {
                *current_window.write().await = Some(initial.clone());
                let _ = tx.send(Some(initial)).await;
            }

            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                break;
                            }
                        }
                        event = event_rx.recv() => {
                            match event {
                                Some(PlatformEvent::WindowChanged(win_opt)) => {
                                    debug!("WindowsTracker received focus change: {:?}", win_opt);
                                    *current_window.write().await = win_opt.clone();
                                    if tx.send(win_opt).await.is_err() {
                                        break;
                                    }
                                }
                                Some(_) => {}
                                None => break,
                            }
                        }
                    }
                }
            });

            self.join_handle = Some(handle);
            Ok(())
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(TrackerError::Tracker(
                "WindowsTracker is only supported on Windows unless configured with mock receiver"
                    .into(),
            ))
        }
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        #[cfg(target_os = "windows")]
        {
            let fg_hwnd =
                unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            if fg_hwnd.is_null() {
                return Ok(None);
            }

            let (real_hwnd, _) = super::win32_utils::resolve_real_process_for_hwnd(fg_hwnd);
            let title = super::win32_utils::get_window_title(real_hwnd);
            let app_class = match super::win32_utils::get_process_path_and_name(real_hwnd) {
                Ok((_, _, name)) => name,
                Err(_) => "unknown".to_string(),
            };

            Ok(Some(WindowInfo { app_class, title }))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(self.current_window.read().await.clone())
        }
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
}
