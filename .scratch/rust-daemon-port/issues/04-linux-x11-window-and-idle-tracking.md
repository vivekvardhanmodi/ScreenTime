# 04: Linux X11 Window Tracking and Idle Detection

**What to build:** Build the Linux X11 tracker and idle backend in `screentimed` using pure-Rust `x11rb`. Listens for root `_NET_ACTIVE_WINDOW` property changes, resolves `WM_CLASS` and `_NET_WM_NAME`, and queries user idle duration via the XScreenSaver extension, with zero C library dependencies (`libxcb-dev`/`libX11-dev`).

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** complete

- [x] Connects to the X11 server via `x11rb::rust_connection::RustConnection` over Unix domain socket
- [x] Interns required atoms (`_NET_ACTIVE_WINDOW`, `_NET_WM_NAME`, `UTF8_STRING`, `WM_CLASS`)
- [x] Subscribes to `EventMask::PROPERTY_CHANGE` on the root window to capture active window switches
- [x] Listens to `PropertyChangeMask` on the active window to detect tab and buffer title updates
- [x] Resolves application class from `WM_CLASS` and window title from `_NET_WM_NAME` (with `WM_NAME` fallback)
- [x] Gracefully catches X11 errors (`BadWindow`) when windows close mid-query
- [x] Queries `x11rb::protocol::screensaver::query_info` to read hardware idle time in milliseconds across all input devices
- [x] Runs the X11 connection on a dedicated OS worker thread streaming `Option<WindowInfo>` and `IdleState` to Tokio MPSC channels
