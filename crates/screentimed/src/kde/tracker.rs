use super::kwin::{load_and_run_kwin_script, unload_kwin_script};
use super::receiver::KdeTrackerReceiver;
use super::script::{generate_kwin_script, resolve_kwin_script_path};
use async_trait::async_trait;
use screentime_core::{TrackerError, WindowInfo, WindowTracker};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use zbus::connection::Builder;
use zbus::export::ordered_stream::OrderedStreamExt;
use zbus::Connection;

pub struct KdeWindowTracker {
    runtime_dir: Option<PathBuf>,
    script_filename: String,
    plugin_name: String,
    service_name: String,
    object_path: String,
    interface_name: String,
    kwin_service: String,
    custom_connection: Option<Connection>,
    custom_address: Option<String>,
    current_window: Arc<Mutex<Option<WindowInfo>>>,
    connection: Option<Connection>,
    script_path: Option<PathBuf>,
    stop_tx: Option<watch::Sender<bool>>,
    lifecycle_handle: Option<JoinHandle<()>>,
}

impl Default for KdeWindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl KdeWindowTracker {
    pub fn new() -> Self {
        Self {
            runtime_dir: None,
            script_filename: "kwin_tracker.js".to_string(),
            plugin_name: "screentime_tracker".to_string(),
            service_name: "org.screentime.Daemon".to_string(),
            object_path: "/org/screentime/Tracker".to_string(),
            interface_name: "org.screentime.Tracker".to_string(),
            kwin_service: "org.kde.KWin".to_string(),
            custom_connection: None,
            custom_address: None,
            current_window: Arc::new(Mutex::new(None)),
            connection: None,
            script_path: None,
            stop_tx: None,
            lifecycle_handle: None,
        }
    }

    pub fn with_runtime_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.runtime_dir = Some(path.into());
        self
    }

    pub fn with_script_filename(mut self, name: impl Into<String>) -> Self {
        self.script_filename = name.into();
        self
    }

    pub fn with_plugin_name(mut self, name: impl Into<String>) -> Self {
        self.plugin_name = name.into();
        self
    }

    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    pub fn with_object_path(mut self, path: impl Into<String>) -> Self {
        self.object_path = path.into();
        self
    }

    pub fn with_interface_name(mut self, name: impl Into<String>) -> Self {
        self.interface_name = name.into();
        self
    }

    pub fn with_kwin_service(mut self, name: impl Into<String>) -> Self {
        self.kwin_service = name.into();
        self
    }

    pub fn with_connection(mut self, conn: Connection) -> Self {
        self.custom_connection = Some(conn);
        self
    }

    pub fn with_bus_address(mut self, address: impl Into<String>) -> Self {
        self.custom_address = Some(address.into());
        self
    }

    pub fn runtime_dir(&self) -> Option<&Path> {
        self.runtime_dir.as_deref()
    }

    pub fn script_path(&self) -> Option<&Path> {
        self.script_path.as_deref()
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn object_path(&self) -> &str {
        &self.object_path
    }

    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }

    pub fn kwin_service(&self) -> &str {
        &self.kwin_service
    }
}

impl Drop for KdeWindowTracker {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(path) = self.script_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[async_trait]
impl WindowTracker for KdeWindowTracker {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        let conn = if let Some(ref conn) = self.custom_connection {
            conn.clone()
        } else if let Some(ref addr) = self.custom_address {
            Builder::address(addr.as_str())
                .map_err(|e| TrackerError::Tracker(format!("Invalid bus address '{addr}': {e}")))?
                .build()
                .await
                .map_err(|e| {
                    TrackerError::Tracker(format!("Failed to connect to bus at '{addr}': {e}"))
                })?
        } else {
            Builder::session()
                .map_err(|e| {
                    TrackerError::Tracker(format!("Failed to configure session bus: {e}"))
                })?
                .build()
                .await
                .map_err(|e| {
                    TrackerError::Tracker(format!("Failed to connect to session bus: {e}"))
                })?
        };

        let receiver = KdeTrackerReceiver::new(tx, Arc::clone(&self.current_window));
        conn.object_server()
            .at(self.object_path.as_str(), receiver)
            .await
            .map_err(|e| {
                TrackerError::Tracker(format!(
                    "Failed to register D-Bus receiver at {}: {e}",
                    self.object_path
                ))
            })?;

        let _ = conn.request_name(self.service_name.as_str()).await;

        let script_path =
            resolve_kwin_script_path(self.runtime_dir.as_deref(), Some(&self.script_filename));
        if let Some(parent) = script_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(TrackerError::Io)?;
        }

        let script_content =
            generate_kwin_script(&self.service_name, &self.object_path, &self.interface_name);

        tokio::fs::write(&script_path, script_content)
            .await
            .map_err(TrackerError::Io)?;

        self.script_path = Some(script_path.clone());
        self.connection = Some(conn.clone());

        if let Err(e) =
            load_and_run_kwin_script(&conn, &self.kwin_service, &script_path, &self.plugin_name)
                .await
        {
            debug!(
                "Initial KWin script injection returned: {e}; will monitor for KWin startup/restart"
            );
        } else {
            info!(
                "Successfully injected KDE KWin tracker script '{}'",
                self.plugin_name
            );
        }

        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let monitor_conn = conn.clone();
        let kwin_service = self.kwin_service.clone();
        let plugin_name = self.plugin_name.clone();

        let handle = tokio::spawn(async move {
            let dbus_proxy = match zbus::fdo::DBusProxy::new(&monitor_conn).await {
                Ok(proxy) => proxy,
                Err(e) => {
                    warn!("Failed to create DBusProxy for KWin monitoring: {e}");
                    return;
                }
            };

            let mut owner_changes = match dbus_proxy.receive_name_owner_changed().await {
                Ok(stream) => stream,
                Err(e) => {
                    warn!("Failed to subscribe to NameOwnerChanged: {e}");
                    return;
                }
            };

            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    signal_opt = owner_changes.next() => {
                        match signal_opt {
                            Some(signal) => {
                                if let Ok(args) = signal.args() {
                                    if args.name.as_str() == kwin_service.as_str()
                                        && args.new_owner.is_some()
                                    {
                                        info!(
                                            "KWin restart detected (service: {}, new owner: {:?}); re-registering script",
                                            kwin_service, args.new_owner
                                        );
                                        tokio::select! {
                                            _ = stop_rx.changed() => {
                                                if *stop_rx.borrow() {
                                                    break;
                                                }
                                            }
                                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {}
                                        }

                                        if let Err(e) = load_and_run_kwin_script(
                                            &monitor_conn,
                                            &kwin_service,
                                            &script_path,
                                            &plugin_name,
                                        ).await {
                                            warn!("Failed to reload KWin script after restart: {e}");
                                        } else {
                                            info!("Re-injected KWin tracker script following restart");
                                        }
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        self.lifecycle_handle = Some(handle);
        Ok(())
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        Ok(self.current_window.lock().await.clone())
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(handle) = self.lifecycle_handle.take() {
            let _ = handle.await;
        }

        if let Some(conn) = self.connection.take() {
            let _ = unload_kwin_script(&conn, &self.kwin_service, &self.plugin_name).await;
            let _ = conn.release_name(self.service_name.as_str()).await;
            let _ = conn
                .object_server()
                .remove::<KdeTrackerReceiver, _>(self.object_path.as_str())
                .await;
        }

        if let Some(script_path) = self.script_path.take() {
            let _ = tokio::fs::remove_file(&script_path).await;
        }

        Ok(())
    }
}
