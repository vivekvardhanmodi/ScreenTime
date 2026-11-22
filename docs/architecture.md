# ScreenTime Daemon Architecture

This document describes the runtime architecture, domain model, IPC protocols, and platform backends of the native ScreenTime daemon.

## System overview

ScreenTime records the active application window and website usage across multiple desktop platforms.
The system stores all session data in a local SQLite database.
The diagram below shows how the components interact.

```mermaid
flowchart TD
    subgraph Browsers
        Chrome[Google Chrome / Chromium]
        Firefox[Mozilla Firefox]
    end

    subgraph NativeMessaging [Browser Host Bridge]
        Host[screentime-browser-host]
    end

    subgraph Daemon [screentimed]
        Platform[Platform Window Tracker]
        Idle[Idle & DPMS Detector]
        Sleep[Systemd Sleep Listener]
        IPC[IPC Unix Socket / Named Pipe]
        Mgr[SessionManager]
        DbActor[DbActor]
    end

    subgraph Storage
        SQLite[(screentime.db)]
    end

    subgraph Frontends
        Web[screentime-web / React]
        TUI[screentime / Terminal UI]
    end

    Chrome -->|stdio JSON| Host
    Firefox -->|stdio JSON| Host
    Host -->|IPC UrlUpdate| IPC
    IPC --> Mgr
    Platform -->|WindowInfo| Mgr
    Idle -->|IdleState| Mgr
    Sleep -->|SleepEvent| Mgr
    Mgr -->|DbCommand| DbActor
    DbActor --> SQLite
    SQLite -.->|read-only SQL| Web
    SQLite -.->|read-only SQL| TUI
```

## Workspace organization

The Rust codebase uses Cargo workspace members to separate foundations from platform drivers.

1. `crates/screentime-core`. Core domain types, database migration logic, actor channels, and shared IPC message serialisation.
2. `crates/screentimed`. The background tracking daemon binary. It manages configuration, platform auto-detection, the event loop, and rolling file logging.
3. `crates/screentime-browser-host`. The native messaging host executable. It translates browser stdin and stdout messages into IPC socket commands for `screentimed`.

## Core domain model

The domain model avoids scattered booleans and mirrors tracking reality.

### Window focus and identity

The tracker identifies active windows using `WindowInfo`.
An active window holds an application identifier and a window title.

```rust
pub struct WindowInfo {
    pub app_class: String,
    pub title: String,
}
```

On Wayland, `app_class` contains the Wayland `app_id`.
On X11, `app_class` contains the `WM_CLASS` class name (falling back to the instance name).
On Windows, `app_class` contains the executable file name, such as `notepad.exe`.

### Idle and power states

User activity transitions through explicit states in `IdleState`.

```rust
pub enum IdleState {
    Active,
    Idle,
}
```

The daemon pauses tracking when:
1. The user provides no keyboard or mouse input beyond the configured timeout.
2. The display server turns monitors off via DPMS.
3. Systemd-logind broadcasts a `PrepareForSleep(true)` D-Bus signal before system suspend (or a Win32 `PBT_APMSUSPEND` power broadcast on Windows).

### Session lifecycle

`SessionManager` reconciles window focus changes, URL updates, and idle transitions.
It guarantees that:
1. Every completed interval writes an immutable start and end timestamp to the database.
2. Walking away stops the clock immediately.
3. Rapid window switching drops transitions shorter than 1.0 second.
4. Active sessions flush to disk upon receiving `SIGTERM` or `SIGINT`.

## IPC and browser integration

ScreenTime captures active web domains in Chrome and Firefox without polling browser tabs.

### Native messaging protocol

The browser launches `screentime-browser-host` as a child process.
Communication uses standard native messaging.
Each packet contains a 4-byte native-endian unsigned length prefix followed by UTF-8 JSON text:

```json
{
  "type": "url_update",
  "source": "chrome",
  "url": "https://github.com/rust-lang/rust",
  "title": "rust-lang/rust"
}
```

### Daemon IPC socket

`screentime-browser-host` forwards newline-delimited JSON `UrlUpdate` messages to `screentimed` over a local channel.
On Linux, it connects to a Unix domain socket at `$XDG_RUNTIME_DIR/screentime/screentimed.sock` (falling back to `/tmp/screentime-<uid>/screentimed.sock`).
On Windows, it connects to the local named pipe `\\.\pipe\screentimed-ipc`.

The daemon accepts URL updates and CLI status probes (which return the running daemon PID) on this socket.
Single-instance execution is enforced using an exclusive OS lock (`flock` on Unix and a Win32 named mutex on Windows) alongside the IPC listener.

## Platform tracking backends

ScreenTime implements platform-specific window tracking via the `WindowTracker` trait.
The daemon auto-detects the running desktop on startup.

```rust
#[async_trait]
pub trait WindowTracker: Send + Sync {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError>;
    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError>;
    async fn stop(&mut self) -> Result<(), TrackerError>;
}
```

### Hyprland

The Hyprland driver connects to `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock` for streaming events and `.socket.sock` for `j/activewindow` / `j/monitors` queries.
It reads event lines including `activewindow>>` and `dpms>>`.
Titles containing commas parse safely by splitting on the first comma only.
When a Wayland display socket is active, idle detection delegates to the Wayland `ext-idle-notify-v1` + `zwlr_output_power_manager_v1` backend; otherwise it falls back to Hyprland socket DPMS tracking.

### KDE Plasma (KWin Wayland)

KWin blocks unprivileged Wayland toplevel inspection.
The KDE driver loads an ECMAScript tracking script through `org.kde.kwin.Scripting` over session D-Bus.
The script registers callbacks on `workspace.windowActivated` (Plasma 6) or `workspace.clientActivated` (Plasma 5).
When focus changes, the script calls `org.screentime.Daemon` (`WindowChanged` on `/org/screentime/Tracker`) over D-Bus.
The daemon receives this call and updates the session manager.
Upon shutdown, the daemon calls `unloadScript` to leave KWin clean.

### wlroots Wayland (Sway, River, Niri)

The wlroots driver speaks the Wayland client wire protocol via `wayland-client`.
It binds to the global `zwlr_foreign_toplevel_manager_v1` interface.
The driver listens for toplevel lifecycle events:
- `zwlr_foreign_toplevel_handle_v1.app_id`
- `zwlr_foreign_toplevel_handle_v1.title`
- `zwlr_foreign_toplevel_handle_v1.state` (filtering for `State::Activated`)

Idle detection connects to `ext_idle_notifier_v1` (honoring idle inhibitors) and `zwlr_output_power_manager_v1` (for DPMS screen power state), automatically handling hot-unplugged outputs/seats and compositor reconnects.

### X11

The X11 driver uses pure-Rust `x11rb` (`RustConnection`) with zero C library dependencies.
It subscribes to `PropertyNotify` events on the root window for `_NET_ACTIVE_WINDOW` and blocks on socket readiness with `poll(2)`.
When active window changes occur, it fetches `WM_CLASS` and `_NET_WM_NAME` (falling back to `WM_NAME`) and watches property changes only on the currently focused window.
Idle detection queries the MIT-SCREEN-SAVER extension via `x11rb::protocol::screensaver::query_info`.

### Windows (Win32)

The Windows driver tracks foreground windows through the Win32 API via `windows-sys`.
A shared message-only window (`HWND_MESSAGE`) thread installs `SetWinEventHook` for `EVENT_SYSTEM_FOREGROUND`, `RegisterPowerSettingNotification` for `GUID_CONSOLE_DISPLAY_STATE`, and `WTSRegisterSessionNotification` for session lock/unlock events.
To handle Universal Windows Platform apps, it resolves child processes behind `ApplicationFrameHost.exe`.
Idle detection calls `GetLastInputInfo` with 32-bit tick rollover protection using adaptive polling (2000 ms while active, tightening to 1000 ms when idle or within 5 seconds of the idle threshold).

## Database architecture and persistence

Data persistence uses an actor model via `DbActor`.
Database writes run on an isolated background thread through an asynchronous bounded channel.
This design prevents SQLite disk I/O from stalling the desktop event loop.

### Migration strategy

The daemon applies schema migrations on startup using `rusqlite_migration` with atomic legacy baselining for existing Python databases (`user_version = 0`).
Migration 1 defines the foundational schema:

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    start_time REAL NOT NULL,
    end_time REAL NOT NULL,
    app_class TEXT NOT NULL,
    app_title TEXT,
    website_url TEXT,
    device_id TEXT DEFAULT 'hyprland-pc'
);

CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_sessions_app ON sessions(app_class);
CREATE INDEX IF NOT EXISTS idx_sessions_url ON sessions(website_url);
CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_unique ON sessions(start_time, app_class, device_id);
```

Frontends access the SQLite file concurrently with WAL mode enabled.

## Sleep and suspend handling

Laptops require explicit suspend handling to avoid recording ghost sessions during sleep.
On Linux, `LogindSleepListener` listens to the system D-Bus signal `org.freedesktop.login1.Manager.PrepareForSleep` and `LidClosed` property changes (respecting clamshell mode when an external display is connected).
On Windows, `WM_POWERBROADCAST` suspend and resume messages trigger the same lifecycle events.
When the machine prepares to sleep, the daemon closes the current session immediately.
When the machine wakes, the daemon re-queries the active window and starts a fresh session.
If system sleep signals are unavailable, the daemon detects time jumps between heartbeat ticks.

## Platform support

The table below lists platform support status.

| Environment | Protocol | Status | Notes |
| :--- | :--- | :--- | :--- |
| **KDE Plasma 6** | KWin Scripting D-Bus | Supported | Injects tracking script via `/Scripting` on session bus. |
| **Sway, River, Niri, Wayfire** | `zwlr_foreign_toplevel_manager_v1` | Supported | Connects to Wayland global foreign toplevel manager. |
| **Openbox, i3, bspwm, XFCE** | EWMH `_NET_ACTIVE_WINDOW` | Supported | Tracks root window property notify events via pure-Rust `x11rb`. |
| **Hyprland** | Hyprland Unix socket | Supported | Streams `activewindow>>` events from `.socket2.sock`. |
| **Windows 10 / 11** | Win32 hooks and power events | Supported | Shared `HWND_MESSAGE` hook thread with `GetLastInputInfo` idle polling. |
| **GNOME Wayland** | Mutter compositor | Not supported | Mutter blocks window inspection from unprivileged clients. |
