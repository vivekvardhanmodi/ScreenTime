# 06: Generic wlroots Wayland Window Tracking and Idle Detection

**What to build:** Build the generic wlroots Wayland window tracker and idle detector in `screentimed` for Sway, River, Wayfire, and Labwc. Uses `zwlr_foreign_toplevel_manager_v1` to track active windows, `ext-idle-notify-v1` for idle detection respecting media/browser inhibitors, and `zwlr_output_power_manager_v1` for instant monitor power-off detection, coordinated over Tokio's `AsyncFd`.

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** complete

- [x] Connects to the Wayland display using `wayland-client` and binds `zwlr_foreign_toplevel_manager_v1`
- [x] Listens for toplevel events, buffering updates and emitting window focus changes on `done` events where `State::Activated` is present
- [x] Binds `ext-idle-notify-v1` from `wayland-protocols` to listen for user idle timeouts, automatically respecting Wayland idle inhibitors (full-screen video, meetings)
- [x] Binds `zwlr_output_power_manager_v1` to listen for display power mode changes (`Mode::Off` vs `Mode::On`) for instant screen blanking transitions
- [x] Integrates the Wayland socket file descriptor with Tokio via `tokio::io::unix::AsyncFd` for cooperative, non-blocking dispatch
- [x] Cleans up all protocol objects and registry bindings on shutdown
