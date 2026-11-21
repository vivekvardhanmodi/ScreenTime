# Linux wlroots and Hyprland Window and Idle Tracking

Type: research
Status: resolved
Blocked by: None

## Question

For Hyprland and wlroots compositors, how should the Rust daemon handle Hyprland's native IPC socket2 event stream (`activewindow>>CLASS,TITLE`) and standard Wayland protocols (`zwlr_foreign_toplevel_manager_v1`, `ext-idle-notify-v1`, `zwlr_output_power_manager_v1`) in Rust (using `wayland-client` / `wayland-protocols`)?

## Answer

Implement two distinct but complementary backends:
1. **Hyprland Native Tracker**: Connect directly to Hyprland's Unix domain sockets (`.socket.sock` for <0.1ms baseline queries and `.socket2.sock` for `activewindow>>CLASS,TITLE` streaming) using pure Tokio without external GPL dependencies.
2. **Generic wlroots / Wayland Tracker**: Use `wayland-client = "0.31"` and `wayland-protocols-wlr = "0.3"` (`zwlr_foreign_toplevel_manager_v1`), buffering attribute updates until `done` events and checking `State::Activated`.
3. **Wayland Idle & Inhibit Detection**: Use `ext-idle-notify-v1` from `wayland-protocols` for user inactivity detection (which automatically respects Wayland `idle-inhibit-unstable-v1` locks from video players and web meetings) and `zwlr_output_power_manager_v1` for instant DPMS monitor off/on notifications.
4. **Tokio Async Integration**: Wrap the Wayland connection in `tokio::io::unix::AsyncFd` for non-blocking event loop integration without dedicated thread pinning. See full research in [.scratch/rust-daemon-port/research/06-linux-wlroots-and-hyprland-window-and-idle-tracking.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/06-linux-wlroots-and-hyprland-window-and-idle-tracking.md).
