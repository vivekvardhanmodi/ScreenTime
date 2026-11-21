## Destination

A complete multi-platform Rust daemon architecture spec and locked ADRs defining the platform abstraction traits (`WindowTracker`, `IdleDetector`, `BrowserHost`, `DatabaseStorage`), with validated research spikes and prototype designs for Windows (Win32), Linux X11 (EWMH), Linux Hyprland (IPC), and Linux KDE (KWin), ready for phased implementation.

## Notes

- **Language & Scope**: Rust daemon (`screentimed`) and browser native messaging host. Python TUI (`screentime.tui`) and Web API remain unchanged and interact via the shared SQLite database.
- **Database Compatibility**: SQLite schema and semantics must remain 100% identical and backward compatible with existing data.
- **Platform Targets in Scope**:
  - Windows 10/11 (Win32 APIs)
  - Linux X11 (EWMH / XScreenSaver)
  - Linux Hyprland (socket2 IPC)
  - Linux KDE Plasma (KWin D-Bus / scripting)
  - Linux Sway / wlroots (`zwlr_foreign_toplevel_manager_v1`, `ext-idle-notify-v1`)
- **Key Skills to Consult**: `domain-modeling`, `codebase-design`, `research`, `prototype`.

## Decisions so far

<!-- the index: one line per closed ticket, enough to judge relevance, then zoom the link for the detail the ticket holds -->

- [Core Daemon Architecture and Async Runtime](.scratch/rust-daemon-port/decisions/01-core-daemon-architecture-and-runtime.md): Tokio multi-thread runtime with actor/channel pattern, socket-bound single-instance lock, and a 3-crate Cargo workspace (`screentime-core`, `screentimed`, `screentime-browser-host`).
- [Database Layer and Schema Compatibility](.scratch/rust-daemon-port/decisions/02-database-layer-and-schema-compatibility.md): Use `rusqlite` (bundled) in a dedicated `DbActor` thread with WAL mode, `rusqlite_migration` automated Python baselining, and in-memory title rules cache.
- [Windows Window and Idle Tracking](.scratch/rust-daemon-port/decisions/03-windows-window-and-idle-tracking.md): Use `windows-sys`, dedicated OS thread with `HWND_MESSAGE`, `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`), `QueryFullProcessImageNameW`, and power/session/idle notification hooks.
- [Linux X11 Window and Idle Tracking](.scratch/rust-daemon-port/decisions/04-linux-x11-window-and-idle-tracking.md): Use pure-Rust `x11rb` without C library dependencies, subscribing to root `_NET_ACTIVE_WINDOW` and querying `XScreenSaverQueryInfo` for hardware idle time.
- [Linux KDE Plasma Window Tracking](.scratch/rust-daemon-port/decisions/05-linux-kde-plasma-window-tracking.md): Dynamically inject a transient ECMAScript helper into KWin via `org.kde.kwin.Scripting` over session D-Bus (`zbus`), receiving real-time `WindowChanged` notifications via `callDBus` without root or polling.
- [Linux wlroots and Hyprland Window and Idle Tracking](.scratch/rust-daemon-port/decisions/06-linux-wlroots-and-hyprland-window-and-idle-tracking.md): Pure Tokio Unix socket streaming for Hyprland (`socket2.sock`), `zwlr_foreign_toplevel_manager_v1` for wlroots compositors, and `ext-idle-notify-v1` with idle-inhibit awareness.
- [Browser Extension Native Messaging Integration](.scratch/rust-daemon-port/decisions/07-browser-extension-native-messaging-integration.md): Build dedicated `screentime-browser-host` binary communicating over local socket IPC (Unix socket/Windows named pipe) via `interprocess`, eliminating polling and stale file hacks.
- [WindowTracker and IdleDetector Trait Design](.scratch/rust-daemon-port/decisions/08-window-tracker-and-idle-detector-trait-design.md): Async traits streaming to Tokio MPSC channels with runtime environment auto-detection for Windows, Hyprland, wlroots, KDE Plasma, and X11.
- [Session Lifecycle and State Machine](.scratch/rust-daemon-port/decisions/09-session-lifecycle-and-state-machine.md): Stateful `SessionManager` coordinating focus changes, noise filtering (<1s), real-time browser URL updates, 5s WAL heartbeats, and graceful shutdown.

## Not yet specified

- Cross-platform daemon autostart & packaging: systemd user service units for Linux distributions vs Windows Startup / Task Scheduler service scripts.
- Hardware power state & lid switch tracking: Linux `systemd-logind` D-Bus signals vs Windows power broadcast notifications.

## Out of scope

- Rewriting Python TUI or Web API in Rust (existing frontend continues reading SQLite).
- GNOME Wayland native window tracking without extensions (Mutter blocks foreign toplevel protocols by design; deferred per user choice).
- macOS platform support (focus is strictly Linux distros and Windows).
- Cloud data synchronization or remote database replication.
