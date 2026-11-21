# Specification: ScreenTime Cross-Platform Rust Daemon Port

Status: ready-for-agent

## Problem Statement

The existing ScreenTime daemon is written in Python and is hardcoded exclusively for Linux running the Hyprland Wayland compositor (`hyprctl` subprocesses and Hyprland IPC `socket2`). Users on other popular desktop environments (KDE Plasma, Sway/wlroots, traditional X11 desktops like Ubuntu/XFCE) and users on Windows cannot use ScreenTime to monitor application and website usage. Furthermore, the Python daemon relies on a 2-second disk-polling loop against temporary JSON files to read active browser URLs, and spawns subprocesses that add runtime overhead and CPU consumption.

## Solution

Port the ScreenTime background tracking daemon (`screentimed`) and the browser native messaging host (`screentime-browser-host`) to Rust as a high-performance, battery-efficient, multi-platform service. 

The Rust daemon provides:
1. Native, push-based window tracking across Windows 10/11, Linux X11, Hyprland, generic wlroots compositors (Sway, River), and KDE Plasma (Wayland) with sub-millisecond event dispatch and near-zero idle CPU usage.
2. Direct local IPC socket streaming for Chrome and Firefox extensions, eliminating file polling and delayed sleep workarounds.
3. 100% SQLite schema and WAL concurrency compatibility with the existing Python TUI (`screentime.tui`) and Web API (`screentime.api`), preserving all user historical data.

## User Stories

1. As a Linux user running Hyprland, I want `screentimed` to connect directly to Hyprland's Unix IPC socket without spawning `hyprctl` subprocesses, so that window focus changes are tracked with zero CPU overhead.
2. As a Linux user running KDE Plasma on Wayland, I want `screentimed` to detect window focus and title changes automatically, so that I can track screen time on my KDE desktop without manual workarounds.
3. As a Linux user running Sway or another wlroots-based compositor, I want `screentimed` to track active toplevel windows via standard Wayland protocols (`zwlr_foreign_toplevel_manager_v1`), so that window tracking works out of the box.
4. As a Linux user running an X11 desktop environment (e.g. Ubuntu, XFCE, Mint), I want `screentimed` to track active windows via pure-Rust EWMH property notifications, so that I can run the daemon without installing C development headers.
5. As a Windows 10 or Windows 11 user, I want `screentimed` to track foreground windows and processes using event hooks, so that my desktop application usage is tracked natively without high polling CPU usage.
6. As a Windows user running modern UWP / Microsoft Store applications (e.g. Settings, Calculator, Photos), I want `screentimed` to resolve the actual target process rather than `ApplicationFrameHost.exe`, so that my application reports accurately reflect what I used.
7. As a laptop user, I want the daemon to instantly pause tracking when my laptop lid closes or when my monitor turns off (DPMS off), so that asleep or locked time is never mistakenly counted as active screen time.
8. As a user watching a full-screen video, playing a game, or attending a conference call, I want the daemon to respect Wayland idle inhibitors, so that continuous media playback is properly counted as active screen time.
9. As a user working on a single task for several hours, I want the daemon to record periodic heartbeat commits every 5 seconds to disk using SQLite WAL mode, so that an unexpected system crash or power outage loses at most a few seconds of tracked time instead of my entire session.
10. As a fast typist who frequently switches between apps using Alt-Tab, I want window focus transitions lasting less than 1.0 second to be filtered out as noise, so that fleeting window switches do not pollute my activity history.
11. As a Chrome or Firefox user, I want active tab URLs to be pushed directly over a local domain socket / named pipe to the daemon, so that my web browsing is attributed to specific domains without delay or file-locking conflicts.
12. As a user with custom terminal or editor window titles, I want the daemon to evaluate user-defined TitleRules with cached, case-insensitive prefix matching, so that projects or terminal tasks are bucketed into their intended logical applications.
13. As an existing ScreenTime user, I want to upgrade to the Rust daemon without losing my existing `screentime.db` database or having to migrate my Python TUI, so that my previous history and dashboards remain completely intact.
14. As a user launching `screentimed`, I want the daemon to enforce a single running instance via socket binding and mutex locks, so that accidental duplicate processes are prevented without stale PID file errors.
15. As a user stopping or restarting the daemon, I want `screentimed` to catch termination signals (`SIGINT`/`SIGTERM` / Console Ctrl) and close the current session with an exact timestamp, so that session durations remain accurate down to the second.

## Implementation Decisions

### 1. Workspace & Architecture Layout
The Rust codebase is organized as a Cargo workspace with three primary crates:
- `crates/screentime-core`: Domain models (`Session`, `WindowInfo`, `IdleState`), SQLite `DbActor` with `rusqlite`, in-memory `TitleRulesCache`, and IPC wire protocol schemas.
- `crates/screentimed`: Main daemon binary containing the central Tokio event loop, runtime platform auto-detection, platform-specific `WindowTracker` and `IdleDetector` backends, and `SessionManager` state machine.
- `crates/screentime-browser-host`: Minimal, standalone native messaging executable reading length-prefixed JSON from browser stdin and streaming URL updates over local IPC to the daemon.

### 2. Database Actor & Schema Compatibility
- **Database Engine**: `rusqlite` with `features = ["bundled"]` isolated inside a dedicated `DbActor` thread communicating via a bounded Tokio MPSC channel (`DbActorHandle`), ensuring single-writer SQLite access without thread pool contention.
- **Connection Pragmas**: Every connection executes:
  - `PRAGMA journal_mode = WAL;`
  - `PRAGMA synchronous = NORMAL;`
  - `PRAGMA busy_timeout = 10000;`
  - `PRAGMA foreign_keys = ON;`
  - `PRAGMA wal_autocheckpoint = 1000;`
- **Migration & Baselining**: Uses `rusqlite_migration`. Existing Python databases (which initialized schema procedurally with `user_version = 0`) are automatically baselined and stamped to `user_version = 1`. New databases execute the canonical schema script.
- **Session Upserts**: Uses atomic SQLite UPSERT (`ON CONFLICT(start_time, app_class, device_id) DO UPDATE SET ... RETURNING id`).

### 3. Unified Platform Trait Interface
Platform-specific tracking is isolated behind two async traits:

```rust
#[async_trait]
pub trait WindowTracker: Send + Sync {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError>;
    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError>;
    async fn stop(&mut self) -> Result<(), TrackerError>;
}

#[async_trait]
pub trait IdleDetector: Send + Sync {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError>;
    fn is_idle(&self) -> bool;
    async fn stop(&mut self) -> Result<(), TrackerError>;
}
```

### 4. Platform Backends
- **Windows**: `windows-sys` crate. Dedicated OS thread creates an invisible message-only window (`HWND_MESSAGE`). `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`) captures focus switches. Process identity is resolved via `QueryFullProcessImageNameW` (with `EnumChildWindows` traversing UWP `ApplicationFrameHost.exe`). User idle time is read via `GetLastInputInfo` (with 32-bit wrapping subtraction). Display power notifications (`GUID_CONSOLE_DISPLAY_STATE`) and session lock notifications (`WTSRegisterSessionNotification`) map to `IdleState`.
- **Linux X11**: Pure-Rust `x11rb` over Unix domain socket with zero C dependencies. Subscribes to `PropertyNotify` on the root window for `_NET_ACTIVE_WINDOW`, resolving `WM_CLASS` and `_NET_WM_NAME`. Hardware idle time is polled via `x11rb::protocol::screensaver::query_info`.
- **Linux Hyprland**: Pure Tokio Unix domain socket client (`.socket2.sock` for `activewindow>>CLASS,TITLE` streaming and `.socket.sock` for `j/activewindow` baseline queries).
- **Linux KDE Plasma (Wayland)**: Dynamically injects a transient ECMAScript helper into KWin via session D-Bus (`org.kde.kwin.Scripting.loadScript`), capturing `workspace.windowActivated` / `clientActivated` and sending `WindowChanged` notifications over D-Bus via `zbus`.
- **Linux wlroots (Sway/River/Wayfire)**: `wayland-client` and `wayland-protocols-wlr` (`zwlr_foreign_toplevel_manager_v1`) coordinated via Tokio's `AsyncFd`. Idle detection uses `ext-idle-notify-v1` (with `idle-inhibit-unstable-v1` awareness) and `zwlr_output_power_manager_v1`.

### 5. Local Socket IPC & Browser Host
- **Transport**: `interprocess::local_socket` (`$XDG_RUNTIME_DIR/screentime/screentimed.sock` on Linux, `\\.\pipe\screentimed-ipc` on Windows).
- **Browser Host Binary**: `screentime-browser-host` compiled as a standalone binary in the workspace. It decodes the browser's 4-byte native messaging length-prefixed JSON on stdin and writes to the local socket in <1ms.
- **Single-Instance Enforcement**: Binding the IPC socket/pipe doubles as the daemon's single-instance lock. On Windows, supplemented with `Global\ScreenTimeDaemonSingleInstance` named mutex.

### 6. Session Lifecycle & State Machine
The `SessionManager` coordinates events inside the Tokio loop:
- Window focus changes close the previous session (discarding if duration $<1.0$s) and open a new session row.
- If a browser window gained focus, `website_url` is updated in-place if the push notification arrives within $2.0$s; subsequent URL changes during an active session split into a new session.
- Heartbeat ticker updates active row `end_time = now` every 5 seconds.
- Idle transitions cleanly close sessions and resume upon active input.

## Testing Decisions

- **Test Seam**: High-level behavioral testing against `DbActorHandle` and `SessionManager` using mock implementations of `WindowTracker` and `IdleDetector`.
- **Database Concurrency & Migrations**: Unit and integration tests verify migration baselining against dummy legacy databases, WAL concurrent read/write transactions, title rule prefix resolution, and noise filter deletion.
- **IPC Protocol Testing**: In-memory duplex stream tests validating length-prefixed native messaging framing, JSON serialization, and socket disconnect resilience.
- **Platform Trait Mocks**: Fuzzing focus switches, rapid alt-tab bursts (<1s), idle/resume sequences, and out-of-order browser URL arrivals against `SessionManager` expectations.

## Out of Scope

- Rewriting the Python TUI (`screentime.tui`) or Web API in Rust (existing frontend continues reading from SQLite).
- GNOME Wayland native window tracking without extensions (deferred until extension/portal ecosystem permits unprivileged foreign toplevel inspection).
- macOS platform support.
- Cloud database syncing or multi-device network replication.

## Further Notes

- The daemon will write logs to `$XDG_STATE_HOME/screentime/screentimed.log` (or Windows `%LOCALAPPDATA%\screentime\logs\screentimed.log`) using `tracing` and `tracing-subscriber`.
- Dynamic platform detection selects the appropriate backend at boot; if an unsupported compositor is detected, it logs an actionable error message and exits cleanly.
