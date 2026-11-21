pub mod cli;
pub mod config;
pub mod ipc;
pub mod logging;
pub mod manifests;
pub mod platform;
pub mod session_manager;
pub mod sleep;

#[cfg(target_os = "linux")]
pub mod hyprland;
#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
pub mod wlroots;
#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(target_os = "windows")]
pub mod windows;

pub use cli::{
    default_db_path, default_log_dir, parse_cli_args, print_help, print_version,
    probe_daemon_status, resolve_socket_path, run_daemon, run_install_manifests, run_main,
    run_status, RunOptions, StatusOptions, Subcommand, VERSION,
};
pub use config::{
    default_config_path, default_data_dir, Config, DaemonConfig, EnvLookup, IniDocument,
    LoggingConfig, PathsConfig, SystemEnv,
};
pub use ipc::{IpcServer, IpcServerError, IpcServerHandle};
pub use logging::{backup_path, init_logging, strip_ansi, RollingFileWriter, ScreenTimeLogWriter};
pub use manifests::{
    default_chrome_dir, default_firefox_dir, default_home_dir, extract_existing_extension_id,
    generate_chrome_manifest, generate_firefox_manifest, install_manifests,
    resolve_browser_host_binary, ChromeManifest, FirefoxManifest, ManifestInstallOptions,
    ManifestInstallResult, DEFAULT_EXTENSION_PLACEHOLDER, DEFAULT_FIREFOX_EXTENSION_ID,
    NATIVE_HOST_NAME,
};
pub use platform::{PlatformBackend, PlatformError};
pub use session_manager::{current_time, is_browser_app, SessionManager};
pub use sleep::{has_external_display, has_external_display_in, is_internal_connector, SleepEvent};

#[cfg(target_os = "linux")]
pub use hyprland::{HyprlandIdleDetector, HyprlandWindowTracker};
#[cfg(target_os = "linux")]
pub use sleep::LogindSleepListener;

#[cfg(target_os = "linux")]
pub use kde::KdeWindowTracker;
#[cfg(target_os = "linux")]
pub use wlroots::{WaylandIdleDetector, WlrForeignToplevelTracker};
#[cfg(target_os = "linux")]
pub use x11::{X11IdleDetector, X11WindowTracker};

#[cfg(target_os = "windows")]
pub use windows::{WindowsIdleDetector, WindowsTracker};
