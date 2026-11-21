# 05: Linux KDE Plasma (Wayland) Window Tracking

**What to build:** Build the KDE Plasma Wayland window tracker in `screentimed`. Dynamically loads a transient ECMAScript helper into KWin via session D-Bus (`org.kde.kwin.Scripting.loadScript`) and receives real-time window focus and title callbacks over D-Bus via `zbus`, operating with zero root/polkit permissions and zero CPU polling.

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** complete

- [x] Daemon registers session D-Bus service `org.screentime.Daemon` at object path `/org/screentime/Tracker` with method `WindowChanged(app_class: s, title: s)` using `zbus`
- [x] Writes a transient polyfill script to `$XDG_RUNTIME_DIR/screentime/kwin_tracker.js` supporting both Plasma 5 (`workspace.clientActivated`) and Plasma 6 (`workspace.windowActivated`)
- [x] Invokes `org.kde.kwin.Scripting.loadScript(path, "screentime_tracker")` over session D-Bus and calls `.run()`
- [x] Listens for `WindowChanged` invocations and forwards `Option<WindowInfo>` to Tokio MPSC channel
- [x] Monitors `org.freedesktop.DBus.NameOwnerChanged` for `org.kde.KWin` to re-inject the script if KWin restarts
- [x] Unloads the script cleanly from KWin upon tracker shutdown
