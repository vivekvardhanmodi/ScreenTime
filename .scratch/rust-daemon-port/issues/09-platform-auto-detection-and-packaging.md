# 09: Platform Auto-Detection, Single-Instance Lock and CLI Packaging

**What to build:** Wire all platform backends into `screentimed` with runtime auto-detection (`PlatformBackend::auto_detect()`), single-instance locking, signal handling (`SIGINT`/`SIGTERM`/Ctrl-C) for clean session commits and socket cleanup, and CLI subcommands/installer scripts for the browser host manifest.

**Blocked by:** 03, 04, 05, 06, 07, 08

**Status:** complete

- [x] `PlatformBackend::auto_detect()` inspects OS and environment variables (`HYPRLAND_INSTANCE_SIGNATURE`, `XDG_CURRENT_DESKTOP`, `WAYLAND_DISPLAY`, `DISPLAY`, `OS`) and selects the appropriate tracker
- [x] Fallback error logging if no supported desktop session is detected
- [x] Single-instance daemon enforcement with clear diagnostic messages if an instance is already running
- [x] Signal handling for graceful shutdown (`tokio::signal::ctrl_c()`, Unix `SIGINT`/`SIGTERM`, Windows Console Ctrl handler), ensuring the active session's end time is recorded in SQLite and sockets/transient scripts are cleaned up
- [x] CLI flags and subcommands: `screentimed --help`, `screentimed --version`, `screentimed status`
- [x] Installer script or helper subcommand to install native messaging manifests for Chrome and Firefox pointing to `screentime-browser-host`
