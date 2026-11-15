# Core Daemon Architecture and Async Runtime

Type: grilling
Status: resolved
Blocked by: None

## Question

What async runtime (e.g. Tokio) and module architecture should `screentimed` use in Rust to orchestrate the event loop, periodic heartbeat updates, signal handling (SIGINT/SIGTERM/SIGHUP on Unix, console control handlers on Windows), single-instance enforcement (PID file/socket/mutex locking), and graceful shutdown across Linux and Windows?

## Answer

1. **Async Runtime**: Tokio multi-thread runtime orchestrating a central event loop via `tokio::select!`. Dedicated background OS worker threads run platform event loops (Win32 message pump, X11 event loop) and the SQLite `DbActor`, streaming events over bounded Tokio MPSC channels. Wayland compositors (Hyprland socket streaming, wlroots AsyncFd) integrate directly with Tokio async I/O.
2. **Single-Instance Enforcement**: IPC socket-bound locking without stale PID file fragility. On Linux, binding `$XDG_RUNTIME_DIR/screentime/screentimed.sock` (with `SO_REUSEADDR` disabled) ensures single-instance execution while serving browser extension IPC. On Windows, a Win32 Global Named Mutex (`Global\ScreenTimeDaemonSingleInstance`) combined with the IPC named pipe `\\.\pipe\screentimed-ipc` enforces single instance.
3. **Workspace Layout**: Standardize on a Cargo workspace:
   - `crates/screentime-core`: Domain models, database actor with `rusqlite`, title rule matching, and IPC wire protocol.
   - `crates/screentimed`: The main background daemon binary with platform-specific trackers and session manager state machine.
   - `crates/screentime-browser-host`: Lightweight, zero-dependency native messaging binary communicating with browsers over stdin/stdout length-prefixed JSON and relaying to the daemon via local socket IPC.
4. **Shutdown & Heartbeats**: Periodic 5s–10s heartbeat timer updates current session `end_time` in SQLite. Graceful shutdown listens for `SIGINT`/`SIGTERM` (Unix) and console Ctrl handler (Windows), closing active sessions with exact timestamps and cleaning up transient sockets/KWin scripts.
