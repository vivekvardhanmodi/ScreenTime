pub mod hyprland;
pub mod ipc;
pub mod session_manager;

#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
pub mod wlroots;
#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(target_os = "windows")]
pub mod windows;

pub use hyprland::{HyprlandIdleDetector, HyprlandWindowTracker};
pub use ipc::{IpcServer, IpcServerError, IpcServerHandle};
pub use session_manager::{current_time, SessionManager};
