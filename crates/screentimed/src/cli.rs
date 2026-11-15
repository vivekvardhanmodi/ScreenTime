use crate::manifests::{default_home_dir, install_manifests, ManifestInstallOptions};
use crate::platform::PlatformBackend;
use crate::{IpcServer, IpcServerError, SessionManager};
use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use screentime_core::ipc::{default_socket_path, socket_name_from_path, UrlUpdate};
use screentime_core::DbActorHandle;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subcommand {
    Run(RunOptions),
    Status(StatusOptions),
    InstallManifests(ManifestInstallOptions),
    Help,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunOptions {
    pub db_path: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
    pub platform: Option<PlatformBackend>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusOptions {
    pub socket_path: Option<PathBuf>,
}

pub fn default_db_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("SCREENTIME_DB_PATH") {
        if !env_path.trim().is_empty() {
            return PathBuf::from(env_path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            return PathBuf::from(app_data)
                .join("screentime")
                .join("screentime.db");
        }
    }

    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("screentime")
            .join("screentime.db");
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data)
                .join("screentime")
                .join("screentime.db");
        }
    }

    default_home_dir()
        .join(".local")
        .join("share")
        .join("screentime")
        .join("screentime.db")
}

pub fn default_log_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            return PathBuf::from(app_data).join("screentime").join("logs");
        }
    }

    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("screentime")
            .join("logs");
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data).join("screentime").join("logs");
        }
    }

    default_home_dir()
        .join(".local")
        .join("share")
        .join("screentime")
        .join("logs")
}

pub fn resolve_socket_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(env_path) = std::env::var("SCREENTIME_SOCKET") {
        if !env_path.trim().is_empty() {
            return PathBuf::from(env_path);
        }
    }
    default_socket_path()
}

pub fn print_help() {
    println!(
        "\
screentimed {VERSION}
Cross-platform screen time tracking daemon

USAGE:
    screentimed [SUBCOMMAND] [OPTIONS]

SUBCOMMANDS:
    (default)            Run the background tracking daemon
    status               Check if screentimed daemon is currently running
    install-manifests    Install Chrome and Firefox native messaging host manifests
    help                 Print this help message
    version              Print version information

OPTIONS:
    --db <PATH>          Path to SQLite database file
    --socket <PATH>      Path to local IPC domain socket / named pipe
    --platform <NAME>    Force platform backend (hyprland, kde, wlroots, x11, windows)
    --device-id <ID>     Device identifier tag for recorded sessions
    --host-path <PATH>   Path to screentime-browser-host binary (install-manifests only)
    --extension-id <ID>  Chrome extension ID (install-manifests only)
    --chrome-dir <DIR>   Custom directory for Chrome native messaging host manifest
    --firefox-dir <DIR>  Custom directory for Firefox native messaging host manifest
    -h, --help           Print help information
    -V, --version        Print version information
"
    );
}

pub fn print_version() {
    println!("screentimed {VERSION}");
}

pub fn parse_cli_args<I, T>(args: I) -> Result<Subcommand, String>
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let raw_args: Vec<String> = args.into_iter().map(|s| s.into()).collect();
    // Drop program binary name if present
    let items = if !raw_args.is_empty()
        && (raw_args[0].ends_with("screentimed")
            || raw_args[0].ends_with("screentimed.exe")
            || raw_args[0].starts_with("target/"))
    {
        &raw_args[1..]
    } else {
        &raw_args[..]
    };

    if items.is_empty() {
        return Ok(Subcommand::Run(RunOptions::default()));
    }

    // Check help / version first
    for item in items {
        if item == "-h" || item == "--help" || item == "help" {
            return Ok(Subcommand::Help);
        }
        if item == "-V" || item == "--version" || item == "version" {
            return Ok(Subcommand::Version);
        }
    }

    let first = &items[0];
    if first == "status" {
        let mut opts = StatusOptions::default();
        let mut i = 1;
        while i < items.len() {
            match items[i].as_str() {
                "--socket" => {
                    i += 1;
                    if i >= items.len() {
                        return Err("--socket requires a path argument".to_string());
                    }
                    opts.socket_path = Some(PathBuf::from(&items[i]));
                }
                other => return Err(format!("Unknown option for 'status': {other}")),
            }
            i += 1;
        }
        return Ok(Subcommand::Status(opts));
    }

    if first == "install-manifests" {
        let mut opts = ManifestInstallOptions::default();
        let mut i = 1;
        while i < items.len() {
            match items[i].as_str() {
                "--host-path" => {
                    i += 1;
                    if i >= items.len() {
                        return Err("--host-path requires a path argument".to_string());
                    }
                    opts.host_path = Some(PathBuf::from(&items[i]));
                }
                "--extension-id" => {
                    i += 1;
                    if i >= items.len() {
                        return Err("--extension-id requires an ID argument".to_string());
                    }
                    opts.extension_id = Some(items[i].clone());
                }
                "--chrome-dir" => {
                    i += 1;
                    if i >= items.len() {
                        return Err("--chrome-dir requires a directory argument".to_string());
                    }
                    opts.chrome_dir = Some(PathBuf::from(&items[i]));
                }
                "--firefox-dir" => {
                    i += 1;
                    if i >= items.len() {
                        return Err("--firefox-dir requires a directory argument".to_string());
                    }
                    opts.firefox_dir = Some(PathBuf::from(&items[i]));
                }
                other => return Err(format!("Unknown option for 'install-manifests': {other}")),
            }
            i += 1;
        }
        return Ok(Subcommand::InstallManifests(opts));
    }

    // Default or "run" subcommand
    let mut run_opts = RunOptions::default();
    let mut i = if first == "run" { 1 } else { 0 };

    while i < items.len() {
        match items[i].as_str() {
            "--db" => {
                i += 1;
                if i >= items.len() {
                    return Err("--db requires a database path argument".to_string());
                }
                run_opts.db_path = Some(PathBuf::from(&items[i]));
            }
            "--socket" => {
                i += 1;
                if i >= items.len() {
                    return Err("--socket requires a socket path argument".to_string());
                }
                run_opts.socket_path = Some(PathBuf::from(&items[i]));
            }
            "--platform" => {
                i += 1;
                if i >= items.len() {
                    return Err("--platform requires a platform backend argument".to_string());
                }
                let backend = items[i]
                    .parse::<PlatformBackend>()
                    .map_err(|e| format!("{e}"))?;
                run_opts.platform = Some(backend);
            }
            "--device-id" => {
                i += 1;
                if i >= items.len() {
                    return Err("--device-id requires an identifier argument".to_string());
                }
                run_opts.device_id = Some(items[i].clone());
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(Subcommand::Run(run_opts))
}

pub async fn probe_daemon_status(socket_path: &Path) -> bool {
    match socket_name_from_path(socket_path) {
        Ok(name) => Stream::connect(name).await.is_ok(),
        Err(_) => false,
    }
}

pub async fn run_status(opts: StatusOptions) -> i32 {
    let socket_path = resolve_socket_path(opts.socket_path.as_deref());
    if probe_daemon_status(&socket_path).await {
        println!(
            "screentimed is running (IPC socket active: {})",
            socket_path.display()
        );
        0
    } else {
        println!("screentimed is not running");
        1
    }
}

pub fn run_install_manifests(opts: ManifestInstallOptions) -> i32 {
    match install_manifests(&opts) {
        Ok(res) => {
            println!("Native messaging manifests installed successfully:");
            println!("  Chrome:  {}", res.chrome_manifest.display());
            println!("  Firefox: {}", res.firefox_manifest.display());
            println!("  Host:    {}", res.host_path.display());
            println!("  Ext ID:  {}", res.extension_id);
            0
        }
        Err(e) => {
            eprintln!("Error installing native messaging manifests: {e}");
            1
        }
    }
}

pub async fn run_daemon(opts: RunOptions) -> i32 {
    let platform = match opts.platform {
        Some(p) => p,
        None => {
            if let Ok(env_p) = std::env::var("SCREENTIME_PLATFORM") {
                if !env_p.trim().is_empty() {
                    match env_p.parse::<PlatformBackend>() {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error parsing $SCREENTIME_PLATFORM: {e}");
                            return 1;
                        }
                    }
                } else {
                    match PlatformBackend::auto_detect() {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error: {e}");
                            return 1;
                        }
                    }
                }
            } else {
                match PlatformBackend::auto_detect() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return 1;
                    }
                }
            }
        }
    };

    info!("Selected platform backend: {platform}");

    // Resolve socket and verify single instance
    let socket_path = resolve_socket_path(opts.socket_path.as_deref());
    let (url_tx, url_rx) = mpsc::channel::<UrlUpdate>(64);

    let mut ipc_handle = match IpcServer::start_with_path(&socket_path, url_tx).await {
        Ok(handle) => handle,
        Err(IpcServerError::AlreadyRunning) => {
            eprintln!("Error: Another instance of screentimed is already running.");
            return 1;
        }
        Err(e) => {
            eprintln!("Error starting IPC server: {e}");
            return 1;
        }
    };

    info!("IPC server listening at {:?}", socket_path);

    // Resolve database
    let db_path = opts.db_path.unwrap_or_else(default_db_path);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    info!("Using SQLite database: {:?}", db_path);
    let db_actor = match DbActorHandle::spawn(&db_path) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("Error opening database at {db_path:?}: {e}");
            ipc_handle.shutdown().await;
            return 1;
        }
    };

    let device_id = opts.device_id.unwrap_or_else(|| {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| format!("{platform}-pc"))
    });

    let session_manager = SessionManager::new(db_actor).with_device_id(device_id);

    let tracker = match platform.create_tracker() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize window tracker: {e}");
            ipc_handle.shutdown().await;
            return 1;
        }
    };

    let idle_detector = match platform.create_idle_detector() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to initialize idle detector: {e}");
            ipc_handle.shutdown().await;
            return 1;
        }
    };

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

    // Spawn signal listener
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = shutdown_tx.send(()).await;
    });

    info!("Starting screentimed event loop...");
    let loop_res = session_manager
        .run_loop(tracker, idle_detector, url_rx, shutdown_rx)
        .await;

    // Shutdown IPC server and clean up socket
    ipc_handle.shutdown().await;

    match loop_res {
        Ok(()) => {
            info!("screentimed shutdown cleanly");
            0
        }
        Err(e) => {
            error!("screentimed event loop error: {e}");
            1
        }
    }
}

pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("Failed to bind SIGINT handler: {e}");
                None
            }
        };
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("Failed to bind SIGTERM handler: {e}");
                None
            }
        };

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl-C signal");
            }
            _ = async {
                if let Some(ref mut s) = sigint {
                    s.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Received Unix SIGINT signal");
            }
            _ = async {
                if let Some(ref mut s) = sigterm {
                    s.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Received Unix SIGTERM signal");
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!("Failed to wait for Ctrl-C: {e}");
        } else {
            info!("Received Console Ctrl / Ctrl-C signal");
        }
    }
}

pub async fn run_main<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    match parse_cli_args(args) {
        Ok(Subcommand::Help) => {
            print_help();
            0
        }
        Ok(Subcommand::Version) => {
            print_version();
            0
        }
        Ok(Subcommand::Status(opts)) => run_status(opts).await,
        Ok(Subcommand::InstallManifests(opts)) => run_install_manifests(opts),
        Ok(Subcommand::Run(opts)) => run_daemon(opts).await,
        Err(err) => {
            eprintln!("Error: {err}\n");
            eprintln!("For more information, try '--help'.");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_and_version() {
        assert_eq!(
            parse_cli_args(vec!["screentimed", "--help"]).unwrap(),
            Subcommand::Help
        );
        assert_eq!(
            parse_cli_args(vec!["screentimed", "-h"]).unwrap(),
            Subcommand::Help
        );
        assert_eq!(
            parse_cli_args(vec!["screentimed", "help"]).unwrap(),
            Subcommand::Help
        );
        assert_eq!(
            parse_cli_args(vec!["screentimed", "--version"]).unwrap(),
            Subcommand::Version
        );
        assert_eq!(
            parse_cli_args(vec!["screentimed", "-V"]).unwrap(),
            Subcommand::Version
        );
    }

    #[test]
    fn test_parse_default_and_run_options() {
        let default_run = parse_cli_args(vec!["screentimed"]).unwrap();
        assert_eq!(default_run, Subcommand::Run(RunOptions::default()));

        let run_args = parse_cli_args(vec![
            "screentimed",
            "run",
            "--db",
            "/tmp/my.db",
            "--socket",
            "/tmp/my.sock",
            "--platform",
            "hyprland",
            "--device-id",
            "my-laptop",
        ])
        .unwrap();

        assert_eq!(
            run_args,
            Subcommand::Run(RunOptions {
                db_path: Some(PathBuf::from("/tmp/my.db")),
                socket_path: Some(PathBuf::from("/tmp/my.sock")),
                platform: Some(PlatformBackend::Hyprland),
                device_id: Some("my-laptop".to_string()),
            })
        );
    }

    #[test]
    fn test_parse_status_options() {
        let status_default = parse_cli_args(vec!["screentimed", "status"]).unwrap();
        assert_eq!(status_default, Subcommand::Status(StatusOptions::default()));

        let status_custom = parse_cli_args(vec![
            "screentimed",
            "status",
            "--socket",
            "/tmp/test_probe.sock",
        ])
        .unwrap();
        assert_eq!(
            status_custom,
            Subcommand::Status(StatusOptions {
                socket_path: Some(PathBuf::from("/tmp/test_probe.sock")),
            })
        );
    }

    #[test]
    fn test_parse_install_manifests_options() {
        let install_cmd = parse_cli_args(vec![
            "screentimed",
            "install-manifests",
            "--host-path",
            "/opt/screentime-browser-host",
            "--extension-id",
            "abcdef123456",
            "--chrome-dir",
            "/tmp/chrome-nmh",
            "--firefox-dir",
            "/tmp/firefox-nmh",
        ])
        .unwrap();

        assert_eq!(
            install_cmd,
            Subcommand::InstallManifests(ManifestInstallOptions {
                host_path: Some(PathBuf::from("/opt/screentime-browser-host")),
                extension_id: Some("abcdef123456".to_string()),
                chrome_dir: Some(PathBuf::from("/tmp/chrome-nmh")),
                firefox_dir: Some(PathBuf::from("/tmp/firefox-nmh")),
            })
        );
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse_cli_args(vec!["screentimed", "--platform"]).is_err());
        assert!(parse_cli_args(vec!["screentimed", "--platform", "invalid"]).is_err());
        assert!(parse_cli_args(vec!["screentimed", "status", "--invalid"]).is_err());
    }
}
