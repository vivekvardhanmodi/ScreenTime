# 02: Core Domain Traits and SessionManager State Machine

**What to build:** Define the canonical `WindowTracker` and `IdleDetector` traits and implement the core `SessionManager` state machine in `screentimed`. It orchestrates active window transitions, debounces idle pauses/resumes, evaluates title rules, discards rapid alt-tab noise ($<1.0$s), handles delayed/in-place browser URL updates, and commits 5-second periodic heartbeats to the database actor.

**Blocked by:** 01 (Workspace Scaffolding and SQLite Database Actor)

**Status:** complete

- [x] `WindowTracker` async trait defined with `start`, `query_active_window`, and `stop`
- [x] `IdleDetector` async trait defined with `start`, `is_idle`, and `stop`
- [x] `SessionManager` receives window changes, deduplicating identical focus events
- [x] Active sessions are closed on window switch; sessions shorter than 1.0 second are deleted as transient noise
- [x] When idle is detected (`IdleState::Idle`), the active session is committed and closed; when active (`IdleState::Active`), a new session is immediately started for the active window
- [x] Periodic 5-second heartbeat timer updates `end_time = now` on the active session in SQLite
- [x] Browser URL updates pushed within 2.0s of focus update the session `website_url` in-place; subsequent URL changes during an active session split into a new session
- [x] Fully verified by unit and integration tests using mock implementations of `WindowTracker` and `IdleDetector`
