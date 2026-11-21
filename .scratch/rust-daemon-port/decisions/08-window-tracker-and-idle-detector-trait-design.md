# WindowTracker and IdleDetector Trait Design

Type: prototype
Status: resolved
Blocked by: 01, 03, 04, 05, 06

## Question

What do the unified `WindowTracker` and `IdleDetector` Rust trait interfaces look like, and how do they decouple platform-specific implementations (Win32, X11, Hyprland, KDE, wlroots) behind clean async event streams or callbacks?

## Answer

Define async traits `WindowTracker` and `IdleDetector` using `tokio::sync::mpsc`:
- `WindowTracker`: `start(tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError>`, `query_active_window() -> Result<Option<WindowInfo>, TrackerError>`, `stop() -> Result<(), TrackerError>`.
- `IdleDetector`: `start(tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError>`, `is_idle() -> bool`, `stop() -> Result<(), TrackerError>`.
- `PlatformBackend::auto_detect()` inspects OS and environment variables (`HYPRLAND_INSTANCE_SIGNATURE`, `XDG_CURRENT_DESKTOP`, `WAYLAND_DISPLAY`, `DISPLAY`, `OS`) to instantiate the matching tracker backend (`Windows`, `Hyprland`, `WlrootsGeneric`, `KdePlasma`, `X11`).
See full prototype in issue discussion and research docs.
