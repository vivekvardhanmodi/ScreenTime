# Linux X11 Window and Idle Tracking

Type: research
Status: resolved
Blocked by: None

## Question

What is the optimal Rust approach (e.g. pure Rust `x11rb` vs `xcb`) for subscribing to `_NET_ACTIVE_WINDOW` property changes on the X11 root window, resolving `WM_CLASS` and `_NET_WM_NAME`, and reading idle time via the XScreenSaver extension (`XScreenSaverQueryInfo`)?

## Answer

Use pure-Rust `x11rb` (`features = ["std", "screensaver", "xproto"]`) over Unix domain sockets with zero C library dependencies (no `libxcb-dev` or `libX11-dev`). Listen for `PropertyNotify` on the root window for `_NET_ACTIVE_WINDOW`, query `WM_CLASS` and `_NET_WM_NAME`, and query user idle time via `x11rb::protocol::screensaver::query_info` (which reports milliseconds since hardware input across all input devices). Run the event loop on a dedicated OS worker thread streaming events to Tokio via unbounded channels. See full research in [.scratch/rust-daemon-port/research/04-linux-x11-window-and-idle-tracking.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/04-linux-x11-window-and-idle-tracking.md).
