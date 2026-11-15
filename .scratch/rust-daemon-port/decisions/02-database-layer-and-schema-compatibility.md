# Database Layer and Schema Compatibility

Type: research
Status: resolved
Blocked by: None

## Question

Which Rust SQLite crate (`rusqlite` with bundled SQLite vs `sqlx-sqlite`) best guarantees 100% schema and WAL concurrency compatibility with the existing Python `sqlite3` database, and how should schema migrations, title rule caching, and session upserts be structured?

## Answer

Use `rusqlite` with `features = ["bundled"]` inside a dedicated `DbActor` thread communicating via a bounded Tokio MPSC channel. SQLite is single-writer in-process; `sqlx-sqlite` pools cause thread contention and lock spikes, whereas a single writer actor guarantees zero internal lock contention and zero blocking on Tokio threads. Run `PRAGMA journal_mode = WAL;`, `synchronous = NORMAL;`, `busy_timeout = 10000;`, and `foreign_keys = ON;` on startup. Schema migrations use `rusqlite_migration` with an automated baselining routine that stamps existing Python installations (which had `user_version = 0`) as `user_version = 1`. Pre-lowercased title rules are cached in memory with a 60s periodic reload, and sessions use atomic UPSERT (`ON CONFLICT(start_time, app_class, device_id) DO UPDATE ... RETURNING id`). See full research in [.scratch/rust-daemon-port/research/02-database-layer-and-schema-compatibility.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/02-database-layer-and-schema-compatibility.md).
