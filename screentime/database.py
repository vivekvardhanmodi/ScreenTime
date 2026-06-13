"""SQLite database operations for ScreenTime.

Stores activity sessions and category mappings with full historical data.
"""

import csv
import sqlite3
import time
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime, date, timedelta
from pathlib import Path
from typing import Optional

from screentime import DB_PATH


@dataclass
class Session:
    """A single activity session."""
    id: int
    start_time: float
    end_time: float
    app_class: str
    app_title: Optional[str]
    website_url: Optional[str]

    @property
    def duration(self) -> float:
        """Duration in seconds."""
        return self.end_time - self.start_time

    @property
    def display_name(self) -> str:
        """Human-readable name: website domain if available, else app class."""
        return self.website_url or self.app_class


@dataclass
class AppUsage:
    """Aggregated usage for an app or website (or a group)."""
    name: str
    identifier_type: str  # 'app', 'website', or 'group'
    total_seconds: float
    category: Optional[str] = None
    children: Optional[list['AppUsage']] = None  # Individual items if this is a group


@dataclass
class Category:
    """A user-defined category."""
    id: int
    name: str


class Database:
    """SQLite database manager for ScreenTime."""

    def __init__(self, db_path: Path = DB_PATH):
        self.db_path = db_path
        self._init_db()

    @contextmanager
    def _connect(self):
        """Context manager for database connections."""
        conn = sqlite3.connect(str(self.db_path), timeout=10)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        conn.row_factory = sqlite3.Row
        try:
            yield conn
            conn.commit()
        except Exception:
            conn.rollback()
            raise
        finally:
            conn.close()

    def _init_db(self):
        """Initialize database schema."""
        with self._connect() as conn:
            # Safely rename website_domain to website_url if it exists
            cursor = conn.execute("PRAGMA table_info(sessions)")
            columns = [row["name"] for row in cursor.fetchall()]
            if "website_domain" in columns:
                try:
                    conn.execute("ALTER TABLE sessions RENAME COLUMN website_domain TO website_url")
                except sqlite3.OperationalError:
                    pass

            conn.executescript("""
                CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    start_time REAL NOT NULL,
                    end_time REAL NOT NULL,
                    app_class TEXT NOT NULL,
                    app_title TEXT,
                    website_url TEXT,
                    website_title TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_time
                    ON sessions(start_time, end_time);
                CREATE INDEX IF NOT EXISTS idx_sessions_app
                    ON sessions(app_class);
                CREATE INDEX IF NOT EXISTS idx_sessions_url
                    ON sessions(website_url);

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

                CREATE INDEX IF NOT EXISTS idx_app_categories_ident
                    ON app_categories(app_identifier);

                CREATE TABLE IF NOT EXISTS app_groups (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    group_name TEXT NOT NULL,
                    app_identifier TEXT NOT NULL UNIQUE,
                    identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website'))
                );

                CREATE INDEX IF NOT EXISTS idx_app_groups_name
                    ON app_groups(group_name);
                CREATE INDEX IF NOT EXISTS idx_app_groups_ident
                    ON app_groups(app_identifier);
            """)

    # ── Title Rules ───────────────────────────────────────────────────

    def get_title_rules(self) -> dict[str, list[str]]:
        """Get all title splitting rules: {app_class: [target_titles]}."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT app_class, target_title FROM title_rules ORDER BY app_class, target_title"
            ).fetchall()
        rules: dict[str, list[str]] = {}
        for r in rows:
            rules.setdefault(r["app_class"], []).append(r["target_title"])
        return rules

    def add_title_rule(self, app_class: str, target_title: str):
        """Add a target title rule for an app."""
        with self._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO title_rules (app_class, target_title) VALUES (?, ?)",
                (app_class, target_title)
            )

    def remove_title_rule(self, app_class: str, target_title: str):
        """Remove a target title rule for an app."""
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM title_rules WHERE app_class = ? AND target_title = ?",
                (app_class, target_title)
            )

    # ── Session operations ────────────────────────────────────────────

    def insert_session(
        self,
        start_time: float,
        end_time: float,
        app_class: str,
        app_title: Optional[str] = None,
        website_url: Optional[str] = None,
    ) -> int:
        """Insert a new session. Returns the session ID."""
        with self._connect() as conn:
            cursor = conn.execute(
                """INSERT INTO sessions
                   (start_time, end_time, app_class, app_title, website_url)
                   VALUES (?, ?, ?, ?, ?)""",
                (start_time, end_time, app_class, app_title, website_url),
            )
            return cursor.lastrowid

    def update_session_end(self, session_id: int, end_time: float):
        """Update the end_time of an existing session (heartbeat)."""
        with self._connect() as conn:
            conn.execute(
                "UPDATE sessions SET end_time = ? WHERE id = ?",
                (end_time, session_id),
            )

    def update_session_website(
        self, session_id: int, website_url: Optional[str]
    ):
        """Update website URL for a session (when browser tab changes without window change)."""
        with self._connect() as conn:
            conn.execute(
                "UPDATE sessions SET website_url = ? WHERE id = ?",
                (website_url, session_id),
            )

    def close_session(self, session_id: int, end_time: float):
        """Close a session by setting its final end_time."""
        self.update_session_end(session_id, end_time)

    def delete_short_sessions(self, min_seconds: float = 1.0):
        """Delete sessions shorter than min_seconds (noise cleanup)."""
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM sessions WHERE (end_time - start_time) < ?",
                (min_seconds,),
            )

    # ── Query operations ──────────────────────────────────────────────

    def _timestamp_range_for_date(self, d: date) -> tuple[float, float]:
        """Get Unix timestamp range for a given date (local timezone)."""
        start = datetime(d.year, d.month, d.day).timestamp()
        end = datetime(d.year, d.month, d.day, 23, 59, 59, 999999).timestamp()
        return start, end

    def get_daily_stats(self, d: date) -> list[AppUsage]:
        """Get per-app/website usage for a specific date."""
        start_ts, end_ts = self._timestamp_range_for_date(d)
        return self._get_stats_in_range(start_ts, end_ts)

    def get_range_stats(self, start_date: date, end_date: date) -> list[AppUsage]:
        """Get aggregated per-app/website usage for a date range (inclusive)."""
        start_ts = datetime(start_date.year, start_date.month, start_date.day).timestamp()
        end_ts = datetime(
            end_date.year, end_date.month, end_date.day, 23, 59, 59, 999999
        ).timestamp()
        return self._get_stats_in_range(start_ts, end_ts)

    def _get_stats_in_range(self, start_ts: float, end_ts: float) -> list[AppUsage]:
        """Get aggregated usage stats for sessions overlapping a time range.

        Handles sessions that span across the range boundaries by clamping.
        Applies app groups: grouped apps are merged into a single entry with
        children containing the individual breakdown. Ungrouped apps appear as-is.
        """
        with self._connect() as conn:
            # Fetch all sessions that overlap with the range
            rows = conn.execute(
                """SELECT app_class, website_url,
                          start_time, end_time
                   FROM sessions
                   WHERE end_time > ? AND start_time < ?
                   ORDER BY start_time""",
                (start_ts, end_ts),
            ).fetchall()

        # Aggregate by effective identifier (website_url or app_class)
        usage_map: dict[tuple[str, str], float] = {}  # (name, type) -> seconds

        for row in rows:
            # Clamp session to the requested range
            effective_start = max(row["start_time"], start_ts)
            effective_end = min(row["end_time"], end_ts)
            duration = effective_end - effective_start

            if duration <= 0:
                continue

            if row["website_url"]:
                key = (row["website_url"], "website")
            else:
                key = (row["app_class"], "app")

            usage_map[key] = usage_map.get(key, 0) + duration

        # Look up categories and groups
        category_map = self._get_category_map()
        group_map = self._get_group_map()  # app_identifier -> group_name

        # Build grouped and ungrouped results
        grouped: dict[str, list[AppUsage]] = {}  # group_name -> [child AppUsage]
        ungrouped: list[AppUsage] = []

        for (name, id_type), total_secs in usage_map.items():
            category = category_map.get(name)
            usage = AppUsage(
                name=name,
                identifier_type=id_type,
                total_seconds=total_secs,
                category=category,
            )

            grp = group_map.get(name)
            if grp:
                grouped.setdefault(grp, []).append(usage)
            else:
                ungrouped.append(usage)

        # Build final result: groups become single entries with children
        result: list[AppUsage] = []

        for grp_name, children in grouped.items():
            children.sort(key=lambda u: u.total_seconds, reverse=True)
            total = sum(c.total_seconds for c in children)
            # Group inherits category from first child if all share the same category
            cats = {c.category for c in children if c.category}
            grp_category = category_map.get(grp_name) or (cats.pop() if len(cats) == 1 else None)
            result.append(AppUsage(
                name=grp_name,
                identifier_type="group",
                total_seconds=total,
                category=grp_category,
                children=children,
            ))

        result.extend(ungrouped)

        # Sort by usage descending
        result.sort(key=lambda u: u.total_seconds, reverse=True)
        return result

    def get_total_time_for_date(self, d: date) -> float:
        """Get total screen time in seconds for a specific date."""
        stats = self.get_daily_stats(d)
        return sum(u.total_seconds for u in stats)

    def get_total_time_in_range(self, start_date: date, end_date: date) -> float:
        """Get total screen time in seconds for a date range."""
        stats = self.get_range_stats(start_date, end_date)
        return sum(u.total_seconds for u in stats)

    def get_daily_totals_in_range(self, start_date: date, end_date: date) -> dict[date, float]:
        """Get per-day total screen time for a date range."""
        result = {}
        current = start_date
        while current <= end_date:
            result[current] = self.get_total_time_for_date(current)
            current += timedelta(days=1)
        return result

    def get_all_app_identifiers(self) -> list[tuple[str, str]]:
        """Get all unique (app_identifier, type) pairs seen in sessions."""
        with self._connect() as conn:
            # Get app classes
            app_rows = conn.execute(
                "SELECT DISTINCT app_class FROM sessions ORDER BY app_class"
            ).fetchall()
            # Get website domains
            web_rows = conn.execute(
                "SELECT DISTINCT website_url FROM sessions WHERE website_url IS NOT NULL ORDER BY website_url"
            ).fetchall()

        result = [(r["app_class"], "app") for r in app_rows]
        result += [(r["website_url"], "website") for r in web_rows]
        return result

    def get_available_dates(self) -> tuple[Optional[date], Optional[date]]:
        """Get the earliest and latest dates with recorded sessions."""
        with self._connect() as conn:
            row = conn.execute(
                "SELECT MIN(start_time) as min_t, MAX(end_time) as max_t FROM sessions"
            ).fetchone()

        if row["min_t"] is None:
            return None, None

        min_date = datetime.fromtimestamp(row["min_t"]).date()
        max_date = datetime.fromtimestamp(row["max_t"]).date()
        return min_date, max_date

    # ── Category operations ───────────────────────────────────────────

    def _get_category_map(self) -> dict[str, str]:
        """Get mapping of app_identifier -> category_name."""
        with self._connect() as conn:
            rows = conn.execute(
                """SELECT ac.app_identifier, c.name
                   FROM app_categories ac
                   JOIN categories c ON ac.category_id = c.id"""
            ).fetchall()
        return {r["app_identifier"]: r["name"] for r in rows}

    def get_categories(self) -> list[Category]:
        """Get all categories."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT id, name FROM categories ORDER BY name"
            ).fetchall()
        return [Category(id=r["id"], name=r["name"]) for r in rows]

    def create_category(self, name: str) -> int:
        """Create a new category. Returns the category ID."""
        with self._connect() as conn:
            cursor = conn.execute(
                "INSERT INTO categories (name) VALUES (?)", (name,)
            )
            return cursor.lastrowid

    def delete_category(self, category_id: int):
        """Delete a category and unassign all its apps."""
        with self._connect() as conn:
            conn.execute("DELETE FROM categories WHERE id = ?", (category_id,))

    def assign_app_to_category(
        self, app_identifier: str, identifier_type: str, category_id: int
    ):
        """Assign an app/website to a category (replaces existing assignment)."""
        with self._connect() as conn:
            conn.execute(
                """INSERT INTO app_categories (app_identifier, identifier_type, category_id)
                   VALUES (?, ?, ?)
                   ON CONFLICT(app_identifier)
                   DO UPDATE SET category_id = excluded.category_id,
                                 identifier_type = excluded.identifier_type""",
                (app_identifier, identifier_type, category_id),
            )

    def unassign_app_from_category(self, app_identifier: str):
        """Remove an app/website from its category."""
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM app_categories WHERE app_identifier = ?",
                (app_identifier,),
            )

    def get_apps_in_category(self, category_id: int) -> list[tuple[str, str]]:
        """Get all (app_identifier, identifier_type) in a category."""
        with self._connect() as conn:
            rows = conn.execute(
                """SELECT app_identifier, identifier_type
                   FROM app_categories WHERE category_id = ?""",
                (category_id,),
            ).fetchall()
        return [(r["app_identifier"], r["identifier_type"]) for r in rows]

    def get_uncategorized_apps(self) -> list[tuple[str, str]]:
        """Get all app identifiers not assigned to any category."""
        all_ids = set(self.get_all_app_identifiers())
        with self._connect() as conn:
            categorized = conn.execute(
                "SELECT app_identifier, identifier_type FROM app_categories"
            ).fetchall()
        categorized_set = {(r["app_identifier"], r["identifier_type"]) for r in categorized}
        uncategorized = sorted(all_ids - categorized_set)
        return uncategorized

    # ── Group operations ───────────────────────────────────────────────

    def _get_group_map(self) -> dict[str, str]:
        """Get mapping of app_identifier -> group_name."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT app_identifier, group_name FROM app_groups"
            ).fetchall()
        return {r["app_identifier"]: r["group_name"] for r in rows}

    def get_groups(self) -> dict[str, list[tuple[str, str]]]:
        """Get all groups: {group_name: [(app_identifier, identifier_type), ...]}."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT group_name, app_identifier, identifier_type FROM app_groups ORDER BY group_name, app_identifier"
            ).fetchall()
        groups: dict[str, list[tuple[str, str]]] = {}
        for r in rows:
            groups.setdefault(r["group_name"], []).append(
                (r["app_identifier"], r["identifier_type"])
            )
        return groups

    def add_to_group(self, group_name: str, app_identifier: str, identifier_type: str):
        """Add an app/website to a group. Creates group if it doesn't exist.
        Moves app to new group if already in one."""
        with self._connect() as conn:
            conn.execute(
                """INSERT INTO app_groups (group_name, app_identifier, identifier_type)
                   VALUES (?, ?, ?)
                   ON CONFLICT(app_identifier)
                   DO UPDATE SET group_name = excluded.group_name,
                                 identifier_type = excluded.identifier_type""",
                (group_name, app_identifier, identifier_type),
            )

    def remove_from_group(self, app_identifier: str):
        """Remove an app/website from its group."""
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM app_groups WHERE app_identifier = ?",
                (app_identifier,),
            )

    def delete_group(self, group_name: str):
        """Delete a group and ungroup all its apps."""
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM app_groups WHERE group_name = ?",
                (group_name,),
            )

    def get_group_names(self) -> list[str]:
        """Get all unique group names."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT DISTINCT group_name FROM app_groups ORDER BY group_name"
            ).fetchall()
        return [r["group_name"] for r in rows]

    # ── Export ─────────────────────────────────────────────────────────

    def export_csv(self, filepath: Path):
        """Export all session data to CSV."""
        category_map = self._get_category_map()

        with self._connect() as conn:
            rows = conn.execute(
                """SELECT * FROM sessions ORDER BY start_time"""
            ).fetchall()

        group_map = self._get_group_map()

        with open(filepath, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow([
                "start_time",
                "end_time",
                "duration_seconds",
                "app_class",
                "app_title",
                "website_url",
                "category",
                "group",
            ])

            for row in rows:
                duration = row["end_time"] - row["start_time"]
                identifier = row["website_url"] or row["app_class"]
                category = category_map.get(identifier, "Uncategorized")
                group = group_map.get(identifier, "")

                writer.writerow([
                    datetime.fromtimestamp(row["start_time"]).isoformat(),
                    datetime.fromtimestamp(row["end_time"]).isoformat(),
                    f"{duration:.1f}",
                    row["app_class"],
                    row["app_title"] or "",
                    row["website_url"] or "",
                    category,
                    group,
                ])
