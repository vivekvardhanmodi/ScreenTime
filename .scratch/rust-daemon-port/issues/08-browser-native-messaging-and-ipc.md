# 08: Local Socket IPC and Browser Native Messaging Host

**What to build:** Build the `screentime-browser-host` binary and the daemon's local socket IPC server using `interprocess`. The standalone host decodes Chrome/Firefox length-prefixed JSON from stdin and streams URL updates over Unix domain sockets / Windows named pipes into `screentimed` in $<1$ms, eliminating file polling and stale state reads.

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** complete

- [x] `screentimed` starts a local socket IPC server (`$XDG_RUNTIME_DIR/screentime/screentimed.sock` on Linux, `\\.\pipe\screentimed-ipc` on Windows) using `interprocess::local_socket`
- [x] Binding the local socket acts as single-instance enforcement (supplemented with Win32 Global Named Mutex on Windows)
- [x] Standalone `screentime-browser-host` binary reads 4-byte native-endian length-prefixed JSON from stdin
- [x] Browser host connects to daemon local socket and pushes `UrlUpdate { url, title, browser_type }` in $<1$ms
- [x] Browser host handles browser disconnections cleanly without zombie processes
- [x] `SessionManager` receives IPC URL updates and reconciles active sessions in-place ($<2.0$s) or splits sessions on navigation
- [x] Zero changes required to `chrome_extension/` or `firefox_extension/` manifests and code
