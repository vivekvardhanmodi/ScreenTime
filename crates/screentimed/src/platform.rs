use screentime_core::{IdleDetector, NoopIdleDetector, TrackerError, WindowTracker};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformBackend {
    Hyprland,
    Kde,
    Wlroots,
    X11,
    Windows,
}

impl fmt::Display for PlatformBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyprland => write!(f, "hyprland"),
            Self::Kde => write!(f, "kde"),
            Self::Wlroots => write!(f, "wlroots"),
            Self::X11 => write!(f, "x11"),
            Self::Windows => write!(f, "windows"),
        }
    }
}

impl FromStr for PlatformBackend {
    type Err = PlatformError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hyprland" => Ok(Self::Hyprland),
            "kde" | "plasma" => Ok(Self::Kde),
            "wlroots" | "wayland" | "sway" | "river" => Ok(Self::Wlroots),
            "x11" | "xcb" | "xorg" => Ok(Self::X11),
            "windows" | "win32" => Ok(Self::Windows),
            other => Err(PlatformError::UnknownPlatform(other.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error(
        "No supported desktop session detected. Expected one of: Hyprland ($HYPRLAND_INSTANCE_SIGNATURE), KDE ($XDG_CURRENT_DESKTOP=KDE), Wayland/wlroots ($WAYLAND_DISPLAY), X11 ($DISPLAY), or Windows"
    )]
    NoSupportedDesktop,

    #[error("Platform backend '{0}' is not supported on this operating system")]
    UnsupportedPlatform(PlatformBackend),

    #[error("Unknown platform name: '{0}' (expected hyprland, kde, wlroots, x11, or windows)")]
    UnknownPlatform(String),

    #[error("Tracker initialization error: {0}")]
    Tracker(#[from] TrackerError),
}

impl PlatformBackend {
    /// Auto-detect the active desktop environment / platform backend inspecting standard environment
    /// variables ($HYPRLAND_INSTANCE_SIGNATURE, $XDG_CURRENT_DESKTOP, $WAYLAND_DISPLAY, $DISPLAY)
    /// and the host operating system.
    pub fn auto_detect() -> Result<Self, PlatformError> {
        let env_map: HashMap<String, String> = std::env::vars().collect();
        let os = std::env::consts::OS;
        Self::auto_detect_from(&env_map, os)
    }

    /// Pure auto-detection logic given an environment map and target OS string.
    pub fn auto_detect_from(
        env: &HashMap<String, String>,
        os: &str,
    ) -> Result<Self, PlatformError> {
        if os == "windows" {
            return Ok(Self::Windows);
        }

        if let Some(sig) = env.get("HYPRLAND_INSTANCE_SIGNATURE") {
            if !sig.trim().is_empty() {
                debug!("Detected Hyprland desktop via HYPRLAND_INSTANCE_SIGNATURE");
                return Ok(Self::Hyprland);
            }
        }

        let xdg_current = env
            .get("XDG_CURRENT_DESKTOP")
            .map(|s| s.to_ascii_uppercase())
            .unwrap_or_default();
        let xdg_session = env
            .get("XDG_SESSION_DESKTOP")
            .map(|s| s.to_ascii_uppercase())
            .unwrap_or_default();

        let is_kde = xdg_current.contains("KDE") || xdg_session.contains("KDE");
        if is_kde {
            debug!("Detected KDE Plasma desktop via XDG_CURRENT_DESKTOP / XDG_SESSION_DESKTOP");
            return Ok(Self::Kde);
        }

        if let Some(wayland_display) = env.get("WAYLAND_DISPLAY") {
            if !wayland_display.trim().is_empty() {
                debug!("Detected Wayland session via WAYLAND_DISPLAY={wayland_display}");
                return Ok(Self::Wlroots);
            }
        }

        if let Some(x11_display) = env.get("DISPLAY") {
            if !x11_display.trim().is_empty() {
                debug!("Detected X11 session via DISPLAY={x11_display}");
                return Ok(Self::X11);
            }
        }

        warn!("No supported desktop environment detected in environment variables");
        Err(PlatformError::NoSupportedDesktop)
    }

    /// Instantiate the WindowTracker for this platform backend.
    pub fn create_tracker(&self) -> Result<Box<dyn WindowTracker>, PlatformError> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Hyprland => {
                info!("Initializing Hyprland window tracker");
                Ok(Box::new(crate::hyprland::HyprlandWindowTracker::new()))
            }
            #[cfg(target_os = "linux")]
            Self::Kde => {
                info!("Initializing KDE Plasma window tracker");
                Ok(Box::new(crate::kde::KdeWindowTracker::new()))
            }
            #[cfg(target_os = "linux")]
            Self::Wlroots => {
                info!("Initializing wlroots Wayland foreign toplevel window tracker");
                Ok(Box::new(crate::wlroots::WlrForeignToplevelTracker::new()))
            }
            #[cfg(target_os = "linux")]
            Self::X11 => {
                info!("Initializing X11 window tracker");
                Ok(Box::new(crate::x11::X11WindowTracker::new()))
            }
            #[cfg(target_os = "windows")]
            Self::Windows => {
                info!("Initializing Windows Win32 event hook tracker");
                Ok(Box::new(crate::windows::WindowsTracker::new()))
            }
            #[allow(unreachable_patterns)]
            _ => Err(PlatformError::UnsupportedPlatform(*self)),
        }
    }

    /// Instantiate the IdleDetector for this platform backend.
    pub fn create_idle_detector(&self) -> Result<Box<dyn IdleDetector>, PlatformError> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Hyprland => {
                info!("Initializing Hyprland idle detector");
                Ok(Box::new(crate::hyprland::HyprlandIdleDetector::new()))
            }
            #[cfg(target_os = "linux")]
            Self::Kde => {
                // KDE Wayland natively supports ext-idle-notify-v1
                if std::env::var("WAYLAND_DISPLAY")
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                {
                    info!("Initializing Wayland idle detector for KDE");
                    Ok(Box::new(crate::wlroots::WaylandIdleDetector::new()))
                } else if std::env::var("DISPLAY")
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                {
                    info!("Initializing X11 idle detector fallback for KDE");
                    Ok(Box::new(crate::x11::X11IdleDetector::new()))
                } else {
                    info!("Initializing no-op idle detector for KDE");
                    Ok(Box::new(NoopIdleDetector))
                }
            }
            #[cfg(target_os = "linux")]
            Self::Wlroots => {
                info!("Initializing Wayland idle detector (ext-idle-notify-v1)");
                Ok(Box::new(crate::wlroots::WaylandIdleDetector::new()))
            }
            #[cfg(target_os = "linux")]
            Self::X11 => {
                info!("Initializing X11 idle detector (screensaver / xss)");
                Ok(Box::new(crate::x11::X11IdleDetector::new()))
            }
            #[cfg(target_os = "windows")]
            Self::Windows => {
                info!("Initializing Windows idle detector");
                Ok(Box::new(crate::windows::WindowsIdleDetector::default()))
            }
            #[allow(unreachable_patterns)]
            _ => Err(PlatformError::UnsupportedPlatform(*self)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_backend_from_str() {
        assert_eq!(
            "hyprland".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Hyprland
        );
        assert_eq!(
            "kde".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Kde
        );
        assert_eq!(
            "plasma".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Kde
        );
        assert_eq!(
            "wlroots".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Wlroots
        );
        assert_eq!(
            "wayland".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Wlroots
        );
        assert_eq!(
            "sway".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Wlroots
        );
        assert_eq!(
            "x11".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::X11
        );
        assert_eq!(
            "windows".parse::<PlatformBackend>().unwrap(),
            PlatformBackend::Windows
        );
        assert!("unknown-desktop".parse::<PlatformBackend>().is_err());
    }

    #[test]
    fn test_auto_detect_windows() {
        let env = HashMap::new();
        let res = PlatformBackend::auto_detect_from(&env, "windows").unwrap();
        assert_eq!(res, PlatformBackend::Windows);
    }

    #[test]
    fn test_auto_detect_hyprland() {
        let mut env = HashMap::new();
        env.insert(
            "HYPRLAND_INSTANCE_SIGNATURE".to_string(),
            "abc123_456".to_string(),
        );
        env.insert("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string());
        let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
        assert_eq!(res, PlatformBackend::Hyprland);
    }

    #[test]
    fn test_auto_detect_kde() {
        let mut env = HashMap::new();
        env.insert("XDG_CURRENT_DESKTOP".to_string(), "KDE".to_string());
        env.insert("WAYLAND_DISPLAY".to_string(), "wayland-0".to_string());
        let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
        assert_eq!(res, PlatformBackend::Kde);
    }

    #[test]
    fn test_auto_detect_wlroots() {
        let mut env = HashMap::new();
        env.insert("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string());
        let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
        assert_eq!(res, PlatformBackend::Wlroots);
    }

    #[test]
    fn test_auto_detect_x11() {
        let mut env = HashMap::new();
        env.insert("DISPLAY".to_string(), ":0".to_string());
        let res = PlatformBackend::auto_detect_from(&env, "linux").unwrap();
        assert_eq!(res, PlatformBackend::X11);
    }

    #[test]
    fn test_auto_detect_fallback_failure() {
        let env = HashMap::new();
        let res = PlatformBackend::auto_detect_from(&env, "linux");
        assert!(matches!(res, Err(PlatformError::NoSupportedDesktop)));
    }
}
