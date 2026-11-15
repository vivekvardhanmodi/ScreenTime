# 01: Workspace Scaffolding and SQLite Database Actor

**What to build:** Set up the Rust Cargo workspace and provide a dedicated, asynchronous SQLite database actor (`DbActor`) that safely connects to `screentime.db`, configures WAL concurrency pragmas, runs migrations to baseline existing Python databases without data loss, caches title rules in-memory, and provides asynchronous CRUD for sessions (inserting, updating heartbeats, noise filtering).

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Cargo workspace configured with crates `screentime-core`, `screentimed`, and `screentime-browser-host`
- [ ] `screentime-core` defines shared domain types: `Session`, `WindowInfo`, `IdleState`, `TitleRule`
- [ ] `DbActor` runs on a dedicated OS thread using `rusqlite` with `features = ["bundled"]`
- [ ] Connection initializes with `PRAGMA journal_mode = WAL;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA busy_timeout = 10000;`, and `PRAGMA foreign_keys = ON;`
- [ ] Automated migration baselines existing Python databases where `user_version = 0`, preserving all legacy history and upgrading schema to version 1
- [ ] `DbActorHandle` provides Tokio channels for `insert_session`, `update_end_time`, `filter_noise_sessions`, and `reload_title_rules`
- [ ] In-memory `TitleRulesCache` evaluates case-insensitive prefix matching against window titles with a periodic reload
- [ ] Integration tests verify concurrent read/write access, WAL persistence, migration baselining against a dummy legacy DB, and noise deletion
