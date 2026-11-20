use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEvent {
    Sleep,
    Wake,
    LidClosed { has_external_display: bool },
    LidOpened,
}

pub fn is_internal_connector(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("edp") || lower.contains("lvds") || lower.contains("dsi")
}

pub fn has_external_display_in(drm_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(drm_dir) else {
        return false;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.starts_with("card") || !name.contains('-') {
            continue;
        }

        if is_internal_connector(&name) {
            continue;
        }

        let status_path = entry.path().join("status");
        if let Ok(status) = std::fs::read_to_string(status_path) {
            if status.trim().eq_ignore_ascii_case("connected") {
                return true;
            }
        }
    }

    false
}

pub fn has_external_display() -> bool {
    has_external_display_in(Path::new("/sys/class/drm"))
}

#[cfg(target_os = "linux")]
pub use linux::LogindSleepListener;

#[cfg(target_os = "linux")]
mod linux {
    use super::{has_external_display, has_external_display_in, SleepEvent};
    use screentime_core::TrackerError;
    use std::path::PathBuf;
    use tokio::sync::mpsc;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;
    use tracing::{debug, info};
    use zbus::export::ordered_stream::OrderedStreamExt;
    use zbus::proxy;

    #[proxy(
        interface = "org.freedesktop.login1.Manager",
        default_service = "org.freedesktop.login1",
        default_path = "/org/freedesktop/login1"
    )]
    pub trait LogindManager {
        #[zbus(signal)]
        fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;

        #[zbus(property)]
        fn lid_closed(&self) -> zbus::Result<bool>;
    }

    pub struct LogindSleepListener {
        stop_tx: Option<watch::Sender<bool>>,
        join_handle: Option<JoinHandle<()>>,
    }

    impl LogindSleepListener {
        pub async fn start(sleep_tx: mpsc::Sender<SleepEvent>) -> Result<Self, TrackerError> {
            let connection = zbus::Connection::system().await.map_err(|e| {
                TrackerError::Tracker(format!("Failed to connect to system D-Bus: {e}"))
            })?;
            Self::start_with_connection(connection, sleep_tx, None).await
        }

        pub async fn start_with_connection(
            connection: zbus::Connection,
            sleep_tx: mpsc::Sender<SleepEvent>,
            drm_path: Option<PathBuf>,
        ) -> Result<Self, TrackerError> {
            Self::start_with_options(
                connection,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                sleep_tx,
                drm_path,
            )
            .await
        }

        pub async fn start_with_options(
            connection: zbus::Connection,
            service: &str,
            path: &str,
            sleep_tx: mpsc::Sender<SleepEvent>,
            drm_path: Option<PathBuf>,
        ) -> Result<Self, TrackerError> {
            let service_owned = service.to_string();
            let path_owned = path.to_string();
            let (stop_tx, mut stop_rx) = watch::channel(false);

            let join_handle = tokio::spawn(async move {
                let proxy = match LogindManagerProxy::builder(&connection)
                    .destination(service_owned.as_str())
                    .and_then(|b| b.path(path_owned.as_str()))
                {
                    Ok(builder) => match builder.build().await {
                        Ok(p) => p,
                        Err(e) => {
                            debug!("Failed to build LogindManagerProxy: {e}");
                            return;
                        }
                    },
                    Err(e) => {
                        debug!("Invalid logind destination or path: {e}");
                        return;
                    }
                };

                let mut last_lid_closed: Option<bool> = None;

                if let Ok(initial_lid) = proxy.lid_closed().await {
                    last_lid_closed = Some(initial_lid);
                    if initial_lid {
                        let has_ext = match &drm_path {
                            Some(p) => has_external_display_in(p),
                            None => has_external_display(),
                        };
                        let _ = sleep_tx
                            .send(SleepEvent::LidClosed {
                                has_external_display: has_ext,
                            })
                            .await;
                    }
                }

                let mut sleep_stream = match proxy.receive_prepare_for_sleep().await {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("Failed to subscribe to PrepareForSleep: {e}");
                        return;
                    }
                };

                let mut lid_stream = proxy.receive_lid_closed_changed().await;

                loop {
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                break;
                            }
                        }
                        signal_opt = OrderedStreamExt::next(&mut sleep_stream) => {
                            match signal_opt {
                                Some(signal) => {
                                    if let Ok(args) = signal.args() {
                                        let is_sleeping = *args.start();
                                        if is_sleeping {
                                            info!("PrepareForSleep(true) received, suspending tracking");
                                            let _ = sleep_tx.send(SleepEvent::Sleep).await;
                                        } else {
                                            info!("PrepareForSleep(false) received, resuming tracking");
                                            let _ = sleep_tx.send(SleepEvent::Wake).await;
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                        change_opt = futures_lite::StreamExt::next(&mut lid_stream) => {
                            match change_opt {
                                Some(change) => {
                                    if let Ok(closed) = change.get().await {
                                        if last_lid_closed != Some(closed) {
                                            last_lid_closed = Some(closed);
                                            if closed {
                                                let has_ext = match &drm_path {
                                                    Some(p) => has_external_display_in(p),
                                                    None => has_external_display(),
                                                };
                                                info!("LidClosed(true) received (external display: {has_ext})");
                                                let _ = sleep_tx
                                                    .send(SleepEvent::LidClosed {
                                                        has_external_display: has_ext,
                                                    })
                                                    .await;
                                            } else {
                                                info!("LidClosed(false) received, lid opened");
                                                let _ = sleep_tx.send(SleepEvent::LidOpened).await;
                                            }
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                }
                debug!("LogindSleepListener event loop terminated");
            });

            Ok(Self {
                stop_tx: Some(stop_tx),
                join_handle: Some(join_handle),
            })
        }

        pub async fn stop(&mut self) -> Result<(), TrackerError> {
            if let Some(stop_tx) = self.stop_tx.take() {
                let _ = stop_tx.send(true);
            }
            if let Some(handle) = self.join_handle.take() {
                let _ = handle.await;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_is_internal_connector() {
        assert!(is_internal_connector("card0-eDP-1"));
        assert!(is_internal_connector("card1-eDP-2"));
        assert!(is_internal_connector("card0-LVDS-1"));
        assert!(is_internal_connector("card0-DSI-1"));

        assert!(!is_internal_connector("card0-HDMI-A-1"));
        assert!(!is_internal_connector("card1-DP-1"));
        assert!(!is_internal_connector("card0-DVI-D-1"));
        assert!(!is_internal_connector("card0-VGA-1"));
        assert!(!is_internal_connector("card0-Virtual-1"));
    }

    #[test]
    fn test_has_external_display_in_mock_sysfs() {
        let temp = tempdir().expect("create tempdir");
        let drm = temp.path();

        assert!(!has_external_display_in(drm));

        let edp_dir = drm.join("card1-eDP-1");
        std::fs::create_dir_all(&edp_dir).unwrap();
        std::fs::write(edp_dir.join("status"), "connected\n").unwrap();
        assert!(!has_external_display_in(drm));

        let hdmi_dir = drm.join("card1-HDMI-A-1");
        std::fs::create_dir_all(&hdmi_dir).unwrap();
        std::fs::write(hdmi_dir.join("status"), "disconnected\n").unwrap();
        assert!(!has_external_display_in(drm));

        std::fs::write(hdmi_dir.join("status"), "connected\n").unwrap();
        assert!(has_external_display_in(drm));
    }
}
