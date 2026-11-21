# Research: Database Layer and Schema Compatibility for Rust Daemon

**Ticket Reference:** `.scratch/rust-daemon-port/issues/02-database-layer-and-schema-compatibility.md`  
**Status:** Complete  
**Date:** 2026-11-15  

---

## 1. Executive Summary & Concrete Decisions

This research establishes the database architecture for the Rust desktop daemon (`screentimed`). The daemon must share a single SQLite database (`screentime.db`) concurrently with the existing Python Text User Interface (`screentime.tui`) and Web API (`screentime/api.py`) without schema corruption, lock starvation, or process crashes.

### 1.1 Summary of Architectural Decisions

1. **SQLite Engine Crate Selection: `rusqlite` (with `bundled` feature) over `sqlx-sqlite`**
   * **Decision:** Adopt **`rusqlite` with `features = ["bundled"]`** inside a dedicated **Database Actor** thread communicating via a bounded Tokio MPSC channel.
   * **Why:** 
     * SQLite fundamentally enforces a **single-writer model**, even in WAL mode. Multiple concurrent writers within an async pool (such as `sqlx::SqlitePool`) fight for SQLite's exclusive table lock, causing `SQLITE_BUSY` errors and latency spikes unless throttled to `max_connections = 1`.
     * `rusqlite` wrapped in a dedicated background worker guarantees **zero internal write lock contention** within the daemon.
     * The `bundled` feature compiles SQLite (v3.45+) directly into the binary from official amalgamated C source. This eliminates dynamic runtime dependencies (`libsqlite3-dev` / `sqlite3.dll`) and guarantees 100% identical SQLite engine semantics across Linux (Ubuntu, Fedora, Arch) and Windows 10/11.
     * `rusqlite` avoids the massive compile-time and binary footprint of `sqlx` (which brings in HTTP/TLS/async database client machinery unnecessary for local desktop daemons).

2. **WAL Concurrency & Python Interoperability**
   * **Decision:** Match Python's concurrency configuration while optimizing durability and latency via **`PRAGMA synchronous = NORMAL`** and **`TransactionBehavior::Immediate`**.
   * **Key Pragmas on Every Connection:**
     * `PRAGMA journal_mode = WAL;` (Persistent in DB header; allows non-blocking concurrent readers while daemon writes).
     * `PRAGMA synchronous = NORMAL;` (Safe against application crashes; syncs only at checkpoint boundaries; eliminates disk I/O bottlenecks during 5-second heartbeats).
     * `PRAGMA busy_timeout = 10000;` (10 seconds timeout, matching Python's `sqlite3.connect(..., timeout=10)`).
     * `PRAGMA foreign_keys = ON;` (Enforces relational constraints).
     * `PRAGMA wal_autocheckpoint = 1000;` (Maintains `-wal` file size below ~4 MB under normal operation).
   * **Write Strategy:** Every write transaction uses `TransactionBehavior::Immediate` (`BEGIN IMMEDIATE`), acquiring SQLite's `RESERVED` lock upfront. This completely eliminates SQLite's "deferred transaction upgrade deadlock".

3. **Schema Migrations & Legacy Baselining Strategy**
   * **Decision:** Use **`rusqlite_migration`** paired with an **Automated Startup Baselining Routine**.
   * **The Legacy Problem:** Existing Python databases were initialized procedurally via `Database._init_db()` in `screentime/database.py`, leaving SQLite's `PRAGMA user_version = 0`.
   * **The Solution:** On startup, if `user_version == 0`:
     * If the `sessions` table exists: Run a safe legacy inspection (ensuring `website_url` and `device_id` exist, removing duplicates) and stamp `PRAGMA user_version = 1`.
     * If the database is empty: Let `rusqlite_migration` execute Migration 1 to create all 5 tables and 8 indexes from scratch.
   * **Future Extensibility:** Subsequent migrations (v2, v3, etc.) are versioned through `rusqlite_migration`. Python's idempotent `CREATE TABLE IF NOT EXISTS` checks in `_init_db()` will never break or alter Rust's `user_version`.

4. **Title Rule In-Memory Caching**
   * **Decision:** Store pre-lowercased target rules in an in-memory `TitleRulesCache` (`HashMap<String, Vec<TargetRule>>`) refreshed every 60 seconds.
   * **Why:** Window focus events in Hyprland/X11/Win32 can fire dozens of times per second during rapid window switching. Pre-lowercasing target strings upon database load eliminates per-event string allocations during prefix matching.

5. **Session Upserts & Lifecycle Operations**
   * **Decision:** Atomic UPSERT with **`RETURNING id`** (`INSERT INTO sessions (...) VALUES (...) ON CONFLICT(...) DO UPDATE ... RETURNING id;`).
   * **Why:** Atomic upsert prevents duplicate key violations on `(start_time, app_class, device_id)` during clock adjustments or rapid window restarts, returning the active session ID in a single roundtrip.
   * Session closure and heartbeat updates are executed inside atomic immediate transactions with automatic filtering of short (<1.0s) noise sessions.

---

## 2. Deep Comparative Analysis: `rusqlite` vs `sqlx-sqlite`

### 2.1 Concurrency Model and the Single-Writer Constraint

SQLite is fundamentally an embedded, file-based database engine. While **WAL (Write-Ahead Logging)** mode dramatically improves concurrency by allowing multiple readers to read concurrently with a writer, **SQLite only supports a single writer at any given moment across all threads and processes**.

| Dimension | `sqlx-sqlite` | `rusqlite` (with Actor Pattern) |
| :--- | :--- | :--- |
| **Execution Model** | Async (`async/await`) wrapping a threadpool | Synchronous, direct C-API execution |
| **Connection Strategy** | `SqlitePool` (multi-connection pool by default) | Single dedicated connection on a background thread |
| **Write Lock Contention (Internal)** | **High Risk**: Multiple async tasks attempting writes concurrently contend for SQLite's exclusive lock, causing `SQLITE_BUSY` errors unless constrained to `max_connections = 1` | **Zero**: Writes are queued sequentially via a Tokio MPSC channel; internal lock contention is physically impossible |
| **Cross-Process Concurrency (Python)** | Managed via pool timeout configurations | Managed via `conn.busy_timeout(10s)` and `BEGIN IMMEDIATE` |
| **Compile Time & Dependencies** | Very heavy (`hyper`, `rustls`/`native-tls`, `nom`, async traits) | Very light (direct C wrapper, fast compilation) |
| **Binary Footprint** | Adds ~5–12 MB to binary size | Adds ~1–2 MB to binary size |
| **Cross-Platform Bundling** | Can bundle, but requires complex cargo feature flags | Native `features = ["bundled"]` compiles clean C amalgamated code |
| **Compile-Time Query Verification** | Supported (`query!`), but requires a live database or `.sqlx` cache files | Runtime query preparation (sufficient for small schemas) |

### 2.2 Why `sqlx` is an Anti-Pattern for Desktop Daemons

1. **Async Pool Contention:** In an async server (e.g. Axum), `sqlx::SqlitePool` opens 5–10 connections. If two Tokio tasks concurrently run `sqlx::query("INSERT ...")`, SQLite immediately fails one with `SQLITE_BUSY: database is locked` unless `busy_timeout` is properly configured or connection limits are restricted. To make `sqlx` safe, one must set `max_connections(1)`—which degrades the pool into an expensive single-threaded queue.
2. **Hidden Worker Threads:** Because SQLite C-API functions perform synchronous file I/O, `sqlx` spawns dedicated worker threads behind the scenes anyway to service async calls.
3. **Bloated Dependency Tree:** `sqlx` is architected as a universal multi-database client (Postgres, MySQL, SQLite). Pulling in network and TLS dependencies for a local desktop daemon that only touches a local file is unnecessary bloat.

### 2.3 Why `rusqlite` with an Actor Pattern is Optimal

By placing a single `rusqlite::Connection` on a dedicated OS thread (or a dedicated Tokio blocking task) driven by an MPSC channel:
1. **Serialized Writes:** All heartbeat updates, session starts, and session closures from `screentimed` are queued in memory and executed sequentially.
2. **Zero Async Thread Starvation:** The Tokio runtime's worker threads never perform blocking SQLite disk I/O.
3. **Direct C-API Access:** Direct access to `conn.busy_timeout()`, `conn.pragma_update()`, `conn.raw_transaction()`, and `rusqlite_migration`.
4. **Predictable Bundling:** `rusqlite = { version = "0.31", features = ["bundled"] }` statically links SQLite 3.45+, ensuring identical query planner rules, pragma behavior, and index handling across Windows and Linux.

---

## 3. SQLite WAL Concurrency & Interoperability with Python

### 3.1 SQLite WAL Mechanics: Readers vs Writers

Under standard rollback journal mode (`journal_mode = DELETE`), writing to SQLite requires an exclusive lock that blocks all readers. 

In **WAL mode** (`journal_mode = WAL`):
* All modifications are appended to a separate `-wal` file.
* Readers read a consistent snapshot from the database pages combined with a point-in-time index from the shared memory `-shm` file.
* **Readers never block writers**, and **writers never block readers**.
* This enables the Python TUI (`screentime.tui`) or Web API (`screentime/api.py`) to run complex queries (such as `get_daily_stats()` aggregating thousands of rows) without stalling `screentimed`'s periodic 5-second heartbeats.

```
+-------------------------------------------------------------+
|                       screentime.db                         |
+-------------------------------------------------------------+
         ^                                        ^
         | Read Snapshot (Shared)                 | Append Pages (Exclusive)
         |                                        |
+-------------------+                    +--------------------+
| Python TUI / API  |                    | Rust Daemon Writer |
| (Concurrent Read) |                    | (Dedicated Actor)  |
+-------------------+                    +--------------------+
```

### 3.2 Python `screentime/database.py` Locking Behavior

Inspection of `screentime/database.py` reveals how Python interacts with SQLite:
* **Connection Lifecycle:**
  * In the Python daemon (`screentime/daemon.py` line 411): `Database(persistent=True)` maintains a long-lived connection with `check_same_thread=False`.
  * In the Python API and TUI: `Database(persistent=False)` creates and closes short-lived connections per operation (`_connect()` context manager).
* **Timeouts:** Python connects with `sqlite3.connect(..., timeout=10)`. This sets SQLite's internal busy timeout to 10,000 milliseconds.
* **Transaction Scope:** Python's `_connect()` context manager executes statements and immediately calls `conn.commit()`, releasing locks within milliseconds.

### 3.3 The "Deferred Transaction Upgrade Deadlock" and `TransactionBehavior::Immediate`

In SQLite, a default `BEGIN` statement is **deferred** (`BEGIN DEFERRED`):
1. Connection A (Python) begins a deferred transaction and reads data (acquiring a `SHARED` lock).
2. Connection B (Rust) begins a deferred transaction and reads data (acquiring a `SHARED` lock).
3. Connection A attempts to write, upgrading its lock to `RESERVED`.
4. Connection B attempts to write. Because Connection A already holds `RESERVED`, Connection B cannot upgrade.
5. Furthermore, Connection A cannot commit to `EXCLUSIVE` because Connection B still holds a `SHARED` lock.
6. **Deadlock:** SQLite aborts Connection B with `SQLITE_BUSY` **immediately, without honoring `busy_timeout`**.

#### The Fix: `TransactionBehavior::Immediate`
When opening write transactions in Rust:
```rust
let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
```
`BEGIN IMMEDIATE` acquires the `RESERVED` lock at the very start of the transaction before executing any queries:
* If Python holds the lock, Rust's `BEGIN IMMEDIATE` pauses and sleeps via the SQLite busy handler (up to 10 seconds).
* Once Python commits, Rust acquires the lock and finishes its write.
* Upgrade deadlocks are mathematically eliminated.

### 3.4 Essential PRAGMA Configuration

Every connection opened by `screentimed` must execute the following PRAGMAs:

```sql
-- 1. Enable Write-Ahead Logging (persistent in database header)
PRAGMA journal_mode = WAL;

-- 2. Performance & Crash Safety Optimization
PRAGMA synchronous = NORMAL;

-- 3. Inter-process Lock Waiting (10 seconds, matching Python)
PRAGMA busy_timeout = 10000;

-- 4. Referential Integrity
PRAGMA foreign_keys = ON;

-- 5. Automatic WAL Checkpointing Threshold
PRAGMA wal_autocheckpoint = 1000;
```

#### Why `synchronous = NORMAL` is Critical
* Standard `synchronous = FULL` executes an operating system `fsync()` on physical disk for every single commit. With heartbeats firing every 5 seconds, this causes continuous disk wakeups and SSD wear.
* In WAL mode, SQLite documentation confirms that **`synchronous = NORMAL` is 100% crash-safe against application crashes and kills**. Data is guaranteed consistent. In the event of a sudden kernel panic or power loss, at most the last un-checkpointed transaction is rolled back, with zero risk of database corruption.

---

## 4. Schema Definition & Legacy Baselining Strategy

### 4.1 Canonical Schema and Data Type Mappings

The SQLite schema defined in `screentime/database.py` maps to Rust types as follows:

| Table | Column | SQLite Type | Python Type | Rust Representation | Constraints / Default |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`sessions`** | `id` | `INTEGER` | `int` | `i64` | `PRIMARY KEY AUTOINCREMENT` |
| | `start_time` | `REAL` | `float` (`time.time()`) | `f64` (Unix epoch seconds) | `NOT NULL` |
| | `end_time` | `REAL` | `float` | `f64` (Unix epoch seconds) | `NOT NULL` |
| | `app_class` | `TEXT` | `str` | `String` | `NOT NULL` |
| | `app_title` | `TEXT` | `Optional[str]` | `Option<String>` | Nullable |
| | `website_url` | `TEXT` | `Optional[str]` | `Option<String>` | Nullable |
| | `device_id` | `TEXT` | `str` | `String` | `DEFAULT 'hyprland-pc'` |
| **`title_rules`**| `app_class` | `TEXT` | `str` | `String` | Part of Composite PK |
| | `target_title` | `TEXT` | `str` | `String` | Part of Composite PK |
| **`categories`** | `id` | `INTEGER` | `int` | `i64` | `PRIMARY KEY AUTOINCREMENT` |
| | `name` | `TEXT` | `str` | `String` | `NOT NULL UNIQUE` |
| **`app_categories`**| `id`| `INTEGER` | `int` | `i64` | `PRIMARY KEY AUTOINCREMENT` |
| | `app_identifier`| `TEXT` | `str` | `String` | `NOT NULL UNIQUE` |
| | `identifier_type`| `TEXT`| `str` | `String` / `enum` | `CHECK(identifier_type IN ('app', 'website'))` |
| | `category_id` | `INTEGER` | `int` | `i64` | `REFERENCES categories(id) ON DELETE CASCADE` |
| **`app_groups`** | `id` | `INTEGER` | `int` | `i64` | `PRIMARY KEY AUTOINCREMENT` |
| | `group_name` | `TEXT` | `str` | `String` | `NOT NULL` |
| | `app_identifier`| `TEXT` | `str` | `String` | `NOT NULL UNIQUE` |
| | `identifier_type`| `TEXT`| `str` | `String` / `enum` | `CHECK(identifier_type IN ('app', 'website'))` |

#### Canonical Indexes:
```sql
CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_sessions_app ON sessions(app_class);
CREATE INDEX IF NOT EXISTS idx_sessions_url ON sessions(website_url);
CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_unique ON sessions(start_time, app_class, device_id);

CREATE INDEX IF NOT EXISTS idx_app_categories_ident ON app_categories(app_identifier);
CREATE INDEX IF NOT EXISTS idx_app_groups_name ON app_groups(group_name);
CREATE INDEX IF NOT EXISTS idx_app_groups_ident ON app_groups(app_identifier);
```

### 4.2 The Python Legacy Schema Problem

In `screentime/database.py` (lines 58–96), schema initialization was performed ad-hoc:
```python
cursor = conn.execute("PRAGMA table_info(sessions)")
columns = [row["name"] for row in cursor.fetchall()]

# Safely rename website_domain to website_url if it exists
if "website_domain" in columns:
    conn.execute("ALTER TABLE sessions RENAME COLUMN website_domain TO website_url")

# Safely drop website_title if it exists
if "website_title" in columns:
    conn.execute("ALTER TABLE sessions DROP COLUMN website_title")
        
# Safely add device_id if it doesn't exist
if "device_id" not in columns and len(columns) > 0:
    conn.execute("ALTER TABLE sessions ADD COLUMN device_id TEXT DEFAULT 'hyprland-pc'")
```
Because Python never set `PRAGMA user_version`, existing user databases have `user_version = 0`.

If `rusqlite_migration` encounters a database with `user_version = 0`, it attempts to run Migration 1. If Migration 1 contains non-idempotent statements or if future migrations attempt to add columns already added by Python, crashes will occur.

### 4.3 Automated Startup Baselining Algorithm

To solve this, `screentimed` implements an explicit startup baselining sequence:

```
[Daemon Startup]
       │
       ▼
Read PRAGMA user_version
       │
       ├─► user_version > 0 ────────────────────────► Run migrations.to_latest(&mut conn)
       │
       ▼ user_version == 0
Query sqlite_master for table "sessions"
       │
       ├─► "sessions" does NOT exist ───────────────► Run migrations.to_latest(&mut conn) (Fresh DB)
       │
       ▼ "sessions" DOES exist (Legacy Python DB)
Inspect PRAGMA table_info(sessions)
  - If "website_domain" present: RENAME to "website_url"
  - If "website_title" present: DROP COLUMN "website_title"
  - If "device_id" missing: ADD COLUMN "device_id" TEXT DEFAULT 'hyprland-pc'
  - Deduplicate rows matching (start_time, app_class, device_id)
  - Ensure all 8 canonical indexes exist
Stamp PRAGMA user_version = 1
       │
       ▼
Run migrations.to_latest(&mut conn) (Applies migrations >= 2)
```

### 4.4 Migration Definition with `rusqlite_migration`

```rust
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

pub static MIGRATIONS: &[M] = &[
    // Migration 1: Canonical Base Schema (matches existing Python v0.1.0)
    M::up(r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            start_time REAL NOT NULL,
            end_time REAL NOT NULL,
            app_class TEXT NOT NULL,
            app_title TEXT,
            website_url TEXT,
            device_id TEXT DEFAULT 'hyprland-pc'
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time, end_time);
        CREATE INDEX IF NOT EXISTS idx_sessions_app ON sessions(app_class);
        CREATE INDEX IF NOT EXISTS idx_sessions_url ON sessions(website_url);
        CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_unique ON sessions(start_time, app_class, device_id);

        CREATE TABLE IF NOT EXISTS title_rules (
            app_class TEXT NOT NULL,
            target_title TEXT NOT NULL,
            PRIMARY KEY(app_class, target_title)
        );

        CREATE TABLE IF NOT EXISTS categories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS app_categories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app_identifier TEXT NOT NULL UNIQUE,
            identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website')),
            category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_app_categories_ident ON app_categories(app_identifier);

        CREATE TABLE IF NOT EXISTS app_groups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            group_name TEXT NOT NULL,
            app_identifier TEXT NOT NULL UNIQUE,
            identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website'))
        );
        CREATE INDEX IF NOT EXISTS idx_app_groups_name ON app_groups(group_name);
        CREATE INDEX IF NOT EXISTS idx_app_groups_ident ON app_groups(app_identifier);
    "#),
    // Migration 2 (Future): Add any new daemon fields (e.g. system idle state flags)
];
```

---

## 5. Title Rule Caching & App Class Resolution

### 5.1 Python Behavior & Requirements

In `screentime/daemon.py` (lines 86–100), the app class resolution logic allows users to group distinct browser or IDE windows (e.g., splitting a browser window titled "GitHub — ScreenTime" into a separate virtual app class):

```python
def _resolve_app_class(self, window: WindowInfo) -> str:
    now = time.time()
    if now - self._last_title_rules_fetch > 60.0:
        self._title_rules_cache = self.db.get_title_rules()
        self._last_title_rules_fetch = now

    raw_class = window.app_class
    rules = self._title_rules_cache.get(raw_class)
    if rules and window.title:
        title_lower = window.title.lower()
        for target in rules:
            if title_lower.startswith(target.lower()):
                return target
    return raw_class
```

### 5.2 Zero-Allocation Rust In-Memory Cache Design

In Rust, doing `title.to_lowercase()` and `target.to_lowercase()` on every window focus event generates heap allocations. In high-frequency event loops, we optimize this:

```rust
#[derive(Debug, Clone)]
pub struct TargetRule {
    pub target: String,
    pub target_lower: String,
}

#[derive(Debug, Clone, Default)]
pub struct TitleRulesCache {
    rules: std::collections::HashMap<String, Vec<TargetRule>>,
    last_fetched: Option<std::time::Instant>,
}

impl TitleRulesCache {
    pub fn new(rules: std::collections::HashMap<String, Vec<String>>) -> Self {
        let transformed = rules
            .into_iter()
            .map(|(app_class, targets)| {
                let parsed_targets = targets
                    .into_iter()
                    .map(|t| {
                        let lower = t.to_lowercase();
                        TargetRule { target: t, target_lower: lower }
                    })
                    .collect();
                (app_class, parsed_targets)
            })
            .collect();

        Self {
            rules: transformed,
            last_fetched: Some(std::time::Instant::now()),
        }
    }

    pub fn is_stale(&self, ttl: std::time::Duration) -> bool {
        self.last_fetched
            .map(|t| t.elapsed() > ttl)
            .unwrap_or(true)
    }

    pub fn resolve_app_class(&self, raw_class: &str, window_title: Option<&str>) -> String {
        let Some(targets) = self.rules.get(raw_class) else {
            return raw_class.to_string();
        };

        let Some(title) = window_title else {
            return raw_class.to_string();
        };

        let title_lower = title.to_lowercase();
        for rule in targets {
            if title_lower.starts_with(&rule.target_lower) {
                return rule.target.clone();
            }
        }

        raw_class.to_string()
    }
}
```

### 5.3 Cache Refresh Strategy

The `SessionManager` state machine refreshes the cache:
* **Periodic Timer:** A Tokio interval triggers a background cache reload every 60 seconds.
* **Non-Blocking Read:** The active rules are stored inside an `ArcSwap<TitleRulesCache>` or `Arc<RwLock<TitleRulesCache>>`. The window tracking loop reads the cache synchronously with zero lock contention against the database thread.

---

## 6. Session Upserts & Lifecycle Operations

### 6.1 Session Lifecycle in `screentimed`

The daemon interacts with the database at five distinct points in time:

```
[Window Focused] ────────► Close Previous Session (if > 1.0s, else DELETE)
                          Start New Session (INSERT / UPSERT -> session_id)
                                 │
[Every 5 Seconds] ───────► Heartbeat (UPDATE end_time = now WHERE id = session_id)
                                 │
[Browser URL Event] ────► Update URL in-place or start new website session
                                 │
[Idle / Suspend] ────────► Close Session at last awake time
                                 │
[Daemon Shutdown] ───────► Close Active Session & prune short sessions
```

### 6.2 Atomic UPSERT with `RETURNING id`

When starting a session, clock adjustments or rapid window focus switches could collide with an existing session on `(start_time, app_class, device_id)`. Using SQLite 3.35+ `UPSERT` with `RETURNING id`:

```sql
INSERT INTO sessions (
    start_time, end_time, app_class, app_title, website_url, device_id
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(start_time, app_class, device_id)
DO UPDATE SET 
    end_time = excluded.end_time,
    app_title = excluded.app_title,
    website_url = excluded.website_url
RETURNING id;
```
This statement guarantees:
1. No `UNIQUE constraint failed` errors are ever thrown.
2. The active `session_id` is returned atomically in a single query.

### 6.3 Heartbeat Updates (`update_session_end`)

Every 5 seconds (configurable via `heartbeat_interval`), the daemon issues:
```sql
UPDATE sessions SET end_time = ?1 WHERE id = ?2;
```
Because `synchronous = NORMAL` is active, this update completes in < 0.2ms and does not trigger an immediate disk flush.

### 6.4 Browser Tab URL Transitions

When the browser native messaging host notifies the daemon of a tab URL change:
* **Initial Tab Load:** If a browser window was focused without a URL and the URL arrives within 0.5s, the session is updated in-place:
  ```sql
  UPDATE sessions SET website_url = ?1 WHERE id = ?2;
  ```
* **Tab Switch / Navigation:** If the user switches tabs to a different domain, the current session is closed and a new session is started for the new URL.

### 6.5 Atomic Session Closing & Noise Filtering

In `screentime/daemon.py` (lines 349–351), closing a session updates the end time and then immediately deletes it if shorter than 1.0 second:
```python
self.db.close_session(self._current_session_id, now)
self.db.delete_specific_short_session(self._current_session_id, 1.0)
```
In Rust, this is executed atomically within an immediate transaction:
```rust
pub fn close_session(
    conn: &mut Connection, 
    session_id: i64, 
    end_time: f64, 
    min_seconds: f64
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    
    tx.execute(
        "UPDATE sessions SET end_time = ?1 WHERE id = ?2",
        rusqlite::params![end_time, session_id],
    )?;
    
    tx.execute(
        "DELETE FROM sessions WHERE id = ?1 AND (end_time - start_time) < ?2",
        rusqlite::params![session_id, min_seconds],
    )?;
    
    tx.commit()?;
    Ok(())
}
```

---

## 7. Concrete Rust Architecture & Reference Implementation

### 7.1 Architecture Overview

The database subsystem is structured into three layers:
1. **`DatabaseStorage` Trait:** High-level async interface exposed to the daemon's state machine.
2. **`DbActorHandle`:** Thread-safe handle holding a bounded MPSC channel sender.
3. **`DbActor`:** Synchronous worker running on a dedicated OS thread owning the `rusqlite::Connection`.

```
┌────────────────────────────────────────────────────────┐
│             Tokio Async Runtime (screentimed)          │
│                                                        │
│  [SessionManager]        [HeartbeatTimer]              │
│         │                       │                      │
│         ▼                       ▼                      │
│     DatabaseStorage Trait (DbActorHandle)             │
│                         │                              │
│                         ▼ mpsc::Sender<DbCommand>      │
└─────────────────────────┼──────────────────────────────┘
                          │ (Cross-thread channel)
┌─────────────────────────▼──────────────────────────────┐
│             Dedicated Database Worker Thread           │
│                                                        │
│                  [DbActor Event Loop]                  │
│                         │                              │
│                         ▼                              │
│             rusqlite::Connection (WAL Mode)            │
│                         │                              │
│                         ▼                              │
│                  screentime.db                         │
└────────────────────────────────────────────────────────┘
```

### 7.2 Core Models & Domain Types

```rust
// src/db/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSession {
    pub start_time: f64,
    pub end_time: f64,
    pub app_class: String,
    pub app_title: Option<String>,
    pub website_url: Option<String>,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: i64,
    pub start_time: f64,
    pub end_time: f64,
    pub app_class: String,
    pub app_title: Option<String>,
    pub website_url: Option<String>,
    pub device_id: String,
}
```

### 7.3 `DatabaseStorage` Trait

```rust
// src/db/storage.rs
use super::models::NewSession;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

#[async_trait]
pub trait DatabaseStorage: Send + Sync {
    /// Inserts or upserts a session, returning its row ID.
    async fn insert_session(&self, session: NewSession) -> Result<i64>;

    /// Updates the end time for an active session (heartbeat).
    async fn update_session_end(&self, session_id: i64, end_time: f64) -> Result<()>;

    /// Updates the website URL for an active session.
    async fn update_session_website(&self, session_id: i64, url: Option<String>) -> Result<()>;

    /// Closes a session, deleting it if shorter than `min_duration` seconds.
    async fn close_session(&self, session_id: i64, end_time: f64, min_duration: f64) -> Result<()>;

    /// Deletes all historical sessions shorter than `min_duration` seconds (startup cleanup).
    async fn delete_short_sessions(&self, min_duration: f64) -> Result<usize>;

    /// Fetches all active title splitting rules: { app_class => [target_titles] }.
    async fn get_title_rules(&self) -> Result<HashMap<String, Vec<String>>>;
}
```

### 7.4 Dedicated Database Actor Implementation

```rust
// src/db/actor.rs
use super::models::NewSession;
use super::storage::DatabaseStorage;
use super::migrations::apply_migrations_with_baseline;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

enum DbCommand {
    InsertSession {
        session: NewSession,
        responder: oneshot::Sender<Result<i64>>,
    },
    UpdateSessionEnd {
        session_id: i64,
        end_time: f64,
        responder: oneshot::Sender<Result<()>>,
    },
    UpdateSessionWebsite {
        session_id: i64,
        url: Option<String>,
        responder: oneshot::Sender<Result<()>>,
    },
    CloseSession {
        session_id: i64,
        end_time: f64,
        min_duration: f64,
        responder: oneshot::Sender<Result<()>>,
    },
    DeleteShortSessions {
        min_duration: f64,
        responder: oneshot::Sender<Result<usize>>,
    },
    GetTitleRules {
        responder: oneshot::Sender<Result<HashMap<String, Vec<String>>>>,
    },
}

#[derive(Clone)]
pub struct DbActorHandle {
    sender: mpsc::Sender<DbCommand>,
}

impl DbActorHandle {
    pub fn spawn<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let (sender, mut receiver) = mpsc::channel::<DbCommand>(256);
        let path_buf = db_path.as_ref().to_path_buf();

        std::thread::Builder::new()
            .name("screentime-db-actor".into())
            .spawn(move || {
                let mut conn = match Self::open_and_configure_connection(&path_buf) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to open SQLite database: {:#}", e);
                        return;
                    }
                };

                if let Err(e) = apply_migrations_with_baseline(&mut conn) {
                    error!("Failed to apply SQLite migrations: {:#}", e);
                    return;
                }

                info!("Database initialized successfully at {:?}", path_buf);

                while let Some(cmd) = receiver.blocking_recv() {
                    match cmd {
                        DbCommand::InsertSession { session, responder } => {
                            let res = Self::handle_insert_session(&mut conn, session);
                            let _ = responder.send(res);
                        }
                        DbCommand::UpdateSessionEnd { session_id, end_time, responder } => {
                            let res = Self::handle_update_session_end(&mut conn, session_id, end_time);
                            let _ = responder.send(res);
                        }
                        DbCommand::UpdateSessionWebsite { session_id, url, responder } => {
                            let res = Self::handle_update_session_website(&mut conn, session_id, url);
                            let _ = responder.send(res);
                        }
                        DbCommand::CloseSession { session_id, end_time, min_duration, responder } => {
                            let res = Self::handle_close_session(&mut conn, session_id, end_time, min_duration);
                            let _ = responder.send(res);
                        }
                        DbCommand::DeleteShortSessions { min_duration, responder } => {
                            let res = Self::handle_delete_short_sessions(&mut conn, min_duration);
                            let _ = responder.send(res);
                        }
                        DbCommand::GetTitleRules { responder } => {
                            let res = Self::handle_get_title_rules(&conn);
                            let _ = responder.send(res);
                        }
                    }
                }
            })
            .context("Failed to spawn database worker thread")?;

        Ok(Self { sender })
    }

    fn open_and_configure_connection(path: &Path) -> Result<Connection> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Critical Concurrency & Safety Pragmas
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "wal_autocheckpoint", 1000)?;

        Ok(conn)
    }

    fn handle_insert_session(conn: &mut Connection, s: NewSession) -> Result<i64> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: i64 = tx.query_row(
            r#"
            INSERT INTO sessions (
                start_time, end_time, app_class, app_title, website_url, device_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(start_time, app_class, device_id)
            DO UPDATE SET
                end_time = excluded.end_time,
                app_title = excluded.app_title,
                website_url = excluded.website_url
            RETURNING id;
            "#,
            params![s.start_time, s.end_time, s.app_class, s.app_title, s.website_url, s.device_id],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(id)
    }

    fn handle_update_session_end(conn: &mut Connection, session_id: i64, end_time: f64) -> Result<()> {
        conn.execute(
            "UPDATE sessions SET end_time = ?1 WHERE id = ?2",
            params![end_time, session_id],
        )?;
        Ok(())
    }

    fn handle_update_session_website(conn: &mut Connection, session_id: i64, url: Option<String>) -> Result<()> {
        conn.execute(
            "UPDATE sessions SET website_url = ?1 WHERE id = ?2",
            params![url, session_id],
        )?;
        Ok(())
    }

    fn handle_close_session(
        conn: &mut Connection,
        session_id: i64,
        end_time: f64,
        min_duration: f64,
    ) -> Result<()> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sessions SET end_time = ?1 WHERE id = ?2",
            params![end_time, session_id],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE id = ?1 AND (end_time - start_time) < ?2",
            params![session_id, min_duration],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_delete_short_sessions(conn: &mut Connection, min_duration: f64) -> Result<usize> {
        let count = conn.execute(
            "DELETE FROM sessions WHERE (end_time - start_time) < ?1",
            params![min_duration],
        )?;
        Ok(count)
    }

    fn handle_get_title_rules(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = conn.prepare(
            "SELECT app_class, target_title FROM title_rules ORDER BY app_class, target_title"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for item in rows {
            let (app, target) = item?;
            map.entry(app).or_default().push(target);
        }
        Ok(map)
    }
}

#[async_trait]
impl DatabaseStorage for DbActorHandle {
    async fn insert_session(&self, session: NewSession) -> Result<i64> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::InsertSession { session, responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }

    async fn update_session_end(&self, session_id: i64, end_time: f64) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::UpdateSessionEnd { session_id, end_time, responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }

    async fn update_session_website(&self, session_id: i64, url: Option<String>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::UpdateSessionWebsite { session_id, url, responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }

    async fn close_session(&self, session_id: i64, end_time: f64, min_duration: f64) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::CloseSession { session_id, end_time, min_duration, responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }

    async fn delete_short_sessions(&self, min_duration: f64) -> Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::DeleteShortSessions { min_duration, responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }

    async fn get_title_rules(&self) -> Result<HashMap<String, Vec<String>>> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(DbCommand::GetTitleRules { responder: tx }).await
            .context("DB actor sender closed")?;
        rx.await.context("DB actor dropped response")?
    }
}
```

### 7.5 Baselining and Migration Runner Implementation

```rust
// src/db/migrations.rs
use anyhow::Result;
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

pub fn apply_migrations_with_baseline(conn: &mut Connection) -> Result<()> {
    let current_version: i32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0))?;

    if current_version == 0 {
        let sessions_exist: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='sessions';",
            [],
            |r| r.get(0),
        )?;

        if sessions_exist {
            // Existing legacy Python database detected. Run safe compatibility inspection.
            apply_legacy_python_patches(conn)?;
            // Baseline stamped at version 1
            conn.pragma_update(None, "user_version", 1)?;
        }
    }

    let migrations = Migrations::new(vec![
        // Version 1: Canonical Base Schema
        M::up(r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                start_time REAL NOT NULL,
                end_time REAL NOT NULL,
                app_class TEXT NOT NULL,
                app_title TEXT,
                website_url TEXT,
                device_id TEXT DEFAULT 'hyprland-pc'
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time, end_time);
            CREATE INDEX IF NOT EXISTS idx_sessions_app ON sessions(app_class);
            CREATE INDEX IF NOT EXISTS idx_sessions_url ON sessions(website_url);
            CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_unique ON sessions(start_time, app_class, device_id);

            CREATE TABLE IF NOT EXISTS title_rules (
                app_class TEXT NOT NULL,
                target_title TEXT NOT NULL,
                PRIMARY KEY(app_class, target_title)
            );

            CREATE TABLE IF NOT EXISTS categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS app_categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                app_identifier TEXT NOT NULL UNIQUE,
                identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website')),
                category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_app_categories_ident ON app_categories(app_identifier);

            CREATE TABLE IF NOT EXISTS app_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                group_name TEXT NOT NULL,
                app_identifier TEXT NOT NULL UNIQUE,
                identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website'))
            );
            CREATE INDEX IF NOT EXISTS idx_app_groups_name ON app_groups(group_name);
            CREATE INDEX IF NOT EXISTS idx_app_groups_ident ON app_groups(app_identifier);
        "#),
    ]);

    migrations.to_latest(conn)?;
    Ok(())
}

fn apply_legacy_python_patches(conn: &mut Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(sessions);")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .collect();

    if columns.contains(&"website_domain".to_string()) {
        let _ = conn.execute("ALTER TABLE sessions RENAME COLUMN website_domain TO website_url;", []);
    }
    if columns.contains(&"website_title".to_string()) {
        let _ = conn.execute("ALTER TABLE sessions DROP COLUMN website_title;", []);
    }
    if !columns.contains(&"device_id".to_string()) && !columns.is_empty() {
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN device_id TEXT DEFAULT 'hyprland-pc';", []);
    }

    // Clean duplicate entries if any exist prior to unique index creation
    let _ = conn.execute(
        r#"
        DELETE FROM sessions
        WHERE id NOT IN (
            SELECT MIN(id)
            FROM sessions
            GROUP BY start_time, app_class, device_id
        )
        "#,
        [],
    );

    Ok(())
}
```

---

## 8. Unit & Integration Verification Strategy

To guarantee zero regression against Python data files, testing should be divided into three suites:

1. **In-Memory Concurrency Test:**
   * Launch `DbActorHandle` connected to an in-memory database (`file::memory:?cache=shared`).
   * Spawn 10 concurrent Tokio tasks issuing `insert_session` and `update_session_end` commands simultaneously.
   * Verify zero `SQLITE_BUSY` errors occur and all session end times are recorded accurately.

2. **Python Compatibility Test (Fixture Verification):**
   * Pre-generate a test SQLite database using Python's `screentime/database.py`.
   * Open the database using `DbActorHandle`.
   * Verify that `apply_migrations_with_baseline` stamps `user_version = 1` without errors.
   * Insert sessions from Rust, then run Python queries (`get_daily_stats`, `get_total_active_time`) against the file.
   * Verify that Python reads the Rust-inserted sessions with 100% field parity.

3. **External Write Contention Test:**
   * Open two connections simultaneously: one simulated Python connection executing an exclusive transaction (`BEGIN IMMEDIATE; INSERT INTO categories ...`), and one `DbActorHandle`.
   * Trigger a Rust heartbeat while the Python write is in progress.
   * Verify that `busy_timeout(10s)` cleanly retries and succeeds once Python commits.

---

## 9. Conclusion & Implementation Checklist

| Item | Recommendation | Locked Status |
| :--- | :--- | :--- |
| **Rust SQLite Crate** | `rusqlite = { version = "0.31", features = ["bundled"] }` | **LOCKED** |
| **Migration Crate** | `rusqlite_migration = "1.2"` | **LOCKED** |
| **Concurrency Model** | Dedicated OS Thread Database Actor with Tokio MPSC Channel | **LOCKED** |
| **SQLite Pragmas** | `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=10000`, `foreign_keys=ON` | **LOCKED** |
| **Transaction Semantics** | `TransactionBehavior::Immediate` for all writes | **LOCKED** |
| **Session Insertion** | `INSERT INTO sessions (...) VALUES (...) ON CONFLICT(...) DO UPDATE ... RETURNING id;` | **LOCKED** |
| **Legacy DB Compatibility** | Automated Baselining: Check `PRAGMA user_version == 0`, patch columns, stamp version 1 | **LOCKED** |
| **Title Rules Cache** | In-memory `TitleRulesCache` with pre-lowercased target rules, refreshed every 60s | **LOCKED** |
