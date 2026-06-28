import csv
from datetime import datetime
from pathlib import Path

from screentime.database import Database

def export_csv(db: Database, filepath: Path):
    """Export all session data to CSV."""
    category_map = db._get_category_map()

    with db._connect() as conn:
        rows = conn.execute(
            "SELECT * FROM sessions ORDER BY start_time"
        ).fetchall()

    group_map = db._get_group_map()

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
