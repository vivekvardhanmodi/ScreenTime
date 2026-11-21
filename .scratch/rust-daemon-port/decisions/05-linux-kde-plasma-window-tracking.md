# Linux KDE Plasma Window Tracking

Type: research
Status: resolved
Blocked by: None

## Question

How can the Rust daemon on KDE Plasma Wayland reliably detect active window changes (app name and title) without root privileges or intrusive dependencies — evaluating KWin D-Bus interfaces (`org.kde.KWin`), KWin scripts, and Plasma Wayland protocols?

## Answer

Use a transient dynamic KWin script paired with bi-directional session D-Bus via `zbus`. Plasma Wayland protocols (`zwlr_foreign_toplevel_manager_v1` not implemented, `ext-foreign-toplevel-list-v1` lacks focus state, `org_kde_plasma_window_management` is exclusively locked by `plasmashell`). The daemon exports an unprivileged D-Bus endpoint `org.screentime.Daemon`, writes a transient polyfill script to `$XDG_RUNTIME_DIR/screentime/kwin_tracker.js`, and loads it via `org.kde.kwin.Scripting.loadScript`. The script hooks `workspace.windowActivated` / `clientActivated` and calls `callDBus(...)` on focus/title changes with 0 idle CPU and zero root/polkit permissions. See full research in [.scratch/rust-daemon-port/research/05-linux-kde-plasma-window-tracking.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/05-linux-kde-plasma-window-tracking.md).
