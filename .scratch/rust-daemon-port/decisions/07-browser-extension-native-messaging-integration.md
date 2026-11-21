# Browser Extension Native Messaging Integration

Type: research
Status: resolved
Blocked by: None
Research Document: `.scratch/rust-daemon-port/research/07-browser-extension-native-messaging-integration.md`

## Question

Should the Chrome/Firefox native messaging host be a sub-command of `screentimed` (e.g. `screentimed browser-host`) or a separate binary, and how should it transmit active tab URLs to the running daemon (shared runtime JSON state files like Python vs a local Unix domain socket / Windows named pipe)?

## Answer

Build a dedicated, minimal binary (`screentime-browser-host`) alongside `screentimed` via Cargo `[[bin]]`. Chrome and Firefox native messaging specifications strictly prohibit command-line arguments in the manifest `path` string (manifest arguments cause `ENOENT`). The host connects to the running daemon via local socket IPC (Unix Domain Socket on Linux at `$XDG_RUNTIME_DIR/screentime/screentimed.sock`, Named Pipe on Windows at `\\.\pipe\screentimed-ipc`) using the `interprocess` crate. This eliminates the legacy 2-second file polling loop, eliminates the 0.5s delayed read hack, automatically handles browser disconnects/crashes when the socket drops, and requires zero changes to the browser extensions. See full research in [.scratch/rust-daemon-port/research/07-browser-extension-native-messaging-integration.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/07-browser-extension-native-messaging-integration.md).
