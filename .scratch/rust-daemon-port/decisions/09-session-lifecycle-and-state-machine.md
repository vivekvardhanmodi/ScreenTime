# Session Lifecycle and State Machine

Type: prototype
Status: resolved
Blocked by: 01, 02, 08

## Question

How should the SessionManager state machine be implemented in Rust to handle active window focus switches, idle debouncing, delayed browser URL reconciliation, user title rules, and crash-resilient periodic heartbeats?

## Answer

Implement a stateful `SessionManager` actor in Rust:
1. **Window Focus Events**: Resolves application class via in-memory title rules cache. Deduplicates identical focus changes. Closes previous session (filtering noise sessions $<1.0$s duration). Starts a new session row in SQLite with `start_time = now` returning session ID.
2. **Idle State**: `IdleState::Idle` pauses tracking and closes current session. `IdleState::Active` resumes tracking, re-queries current active window from `WindowTracker::query_active_window()`, and starts a fresh session.
3. **Browser URL Push**: Pushed in real-time over local socket IPC from `screentime-browser-host`. If the session is $<2.0$s old (initial browser tab focus), updates `website_url` in-place on the active row. If the user navigated during an existing session, splits into a new session.
4. **Heartbeat & WAL Persistence**: Periodic 5-second ticker updates `end_time = now` on the active row via `DbActor`. SQLite WAL mode appends sequentially to `screentime.db-wal`, ensuring crash resilience without disk bottlenecks.
5. **Graceful Shutdown**: Traps `SIGTERM`/`SIGINT` / Windows console break, commits active session end timestamp, and unlinks socket/KWin scripts.
