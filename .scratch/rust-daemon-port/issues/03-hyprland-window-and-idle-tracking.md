# 03: Hyprland Window and Idle Tracking

**What to build:** Build the native Hyprland tracker and idle backend in `screentimed`. Connects directly to Hyprland's Unix IPC socket2 for streaming real-time window changes and socket1 for sub-millisecond baseline active window queries, with DPMS idle detection, without external subprocesses or GPL dependencies.

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** complete

- [x] Resolves Hyprland socket paths using `$HYPRLAND_INSTANCE_SIGNATURE` and `$XDG_RUNTIME_DIR` (with `/tmp` fallback)
- [x] Connects to `.socket2.sock` and parses `activewindow>>CLASS,TITLE` streaming events with zero subprocess forks
- [x] Correctly splits on the first comma to handle window titles containing commas, and maps empty class or `activewindow>>,` to `None` (unfocused)
- [x] Implements `query_active_window` by writing `j/activewindow\n` to `.socket.sock` and parsing JSON response
- [x] Implements Hyprland DPMS monitor status tracking (`dpms>>` on socket2 or `j/monitors` on socket1) emitting `IdleState` transitions
- [x] Integration tests verify socket message parsing, reconnection resilience, and simulated event streams against `SessionManager`
