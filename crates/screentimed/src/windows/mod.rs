pub mod events;
pub mod idle;
pub mod math;
pub mod tracker;
pub mod uwp;

#[cfg(target_os = "windows")]
pub mod listener;
#[cfg(target_os = "windows")]
pub mod win32_utils;

#[allow(unused_imports)]
pub use events::PlatformEvent;
pub use idle::WindowsIdleDetector;
pub use tracker::WindowsTracker;
