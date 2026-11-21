use screentime_core::TrackerError;
use std::path::Path;
use tracing::{debug, warn};
use zbus::proxy;

#[proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
pub trait KWinScripting {
    #[zbus(name = "loadScript")]
    fn load_script(&self, file_path: &str, plugin_name: &str) -> zbus::Result<i32>;
    #[zbus(name = "unloadScript")]
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
    #[zbus(name = "start")]
    fn start(&self) -> zbus::Result<()>;
}

#[proxy(interface = "org.kde.kwin.Script", default_service = "org.kde.KWin")]
pub trait KWinScript {
    #[zbus(name = "run")]
    fn run(&self) -> zbus::Result<()>;
    #[zbus(name = "stop")]
    fn stop(&self) -> zbus::Result<()>;
}

pub async fn load_and_run_kwin_script(
    connection: &zbus::Connection,
    kwin_service: &str,
    script_path: &Path,
    plugin_name: &str,
) -> Result<(), TrackerError> {
    let scripting_proxy = KWinScriptingProxy::builder(connection)
        .destination(kwin_service)
        .map_err(|e| TrackerError::Tracker(format!("Invalid KWin service destination: {e}")))?
        .build()
        .await
        .map_err(|e| TrackerError::Tracker(format!("Failed to connect to KWin scripting: {e}")))?;

    let _ = scripting_proxy.unload_script(plugin_name).await;

    let path_str = script_path
        .to_str()
        .ok_or_else(|| TrackerError::Tracker("Invalid script path utf-8".to_string()))?;

    let script_id = scripting_proxy
        .load_script(path_str, plugin_name)
        .await
        .map_err(|e| TrackerError::Tracker(format!("Failed to load KWin script: {e}")))?;

    if script_id < 0 {
        return Err(TrackerError::Tracker(format!(
            "KWin loadScript returned error code: {script_id}"
        )));
    }

    debug!("Loaded KWin script '{plugin_name}' with id {script_id}");

    let plasma6_path = format!("/Scripting/Script{script_id}");
    if let Ok(builder) = KWinScriptProxy::builder(connection)
        .destination(kwin_service)
        .and_then(|b| b.path(plasma6_path.as_str()))
    {
        if let Ok(script_proxy) = builder.build().await {
            if script_proxy.run().await.is_ok() {
                debug!("Executed KWin script via Plasma 6 path {plasma6_path}");
                return Ok(());
            }
        }
    }

    let plasma5_path = format!("/{script_id}");
    if let Ok(builder) = KWinScriptProxy::builder(connection)
        .destination(kwin_service)
        .and_then(|b| b.path(plasma5_path.as_str()))
    {
        if let Ok(script_proxy) = builder.build().await {
            if script_proxy.run().await.is_ok() {
                debug!("Executed KWin script via Plasma 5 path {plasma5_path}");
                return Ok(());
            }
        }
    }

    warn!("Falling back to scripting.start()");
    scripting_proxy
        .start()
        .await
        .map_err(|e| TrackerError::Tracker(format!("Failed to start KWin scripts: {e}")))?;

    Ok(())
}

pub async fn unload_kwin_script(
    connection: &zbus::Connection,
    kwin_service: &str,
    plugin_name: &str,
) -> Result<(), TrackerError> {
    if let Ok(builder) = KWinScriptingProxy::builder(connection).destination(kwin_service) {
        if let Ok(scripting_proxy) = builder.build().await {
            let _ = scripting_proxy.unload_script(plugin_name).await;
            debug!("Unloaded KWin script '{plugin_name}'");
        }
    }
    Ok(())
}
