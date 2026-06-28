import asyncio
from datetime import date, datetime, timedelta
from typing import Dict, List, Optional
import time

from fastapi import FastAPI, HTTPException, Body
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse
from pydantic import BaseModel
import uvicorn
import os

from contextlib import asynccontextmanager

from screentime.database import Database
from screentime.models import AppUsage

@asynccontextmanager
async def lifespan(app: FastAPI):
    global db
    import screentime
    screentime.init_env()
    db = Database(persistent=False)
    yield

app = FastAPI(title="ScreenTime API", lifespan=lifespan)

# Enable CORS for the React dev server
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # Allow all during dev
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Initialize DB connection on startup
db: Database = None

# --- Models ---

class CategoryMapRequest(BaseModel):
    name: str
    identifier_type: str = "app"
    category: str

class CategoryDeleteRequest(BaseModel):
    name: str

class GroupMapRequest(BaseModel):
    name: str
    identifier_type: str = "app"
    group: str

class GroupDeleteRequest(BaseModel):
    name: str

class RuleRequest(BaseModel):
    app_class: str
    target_title: str

class SyncSession(BaseModel):
    start_time: float
    end_time: float
    app_class: str
    app_title: Optional[str] = None
    website_url: Optional[str] = None

class SyncRequest(BaseModel):
    device_id: str
    sessions: List[SyncSession]

# --- Endpoints ---

@app.get("/api/summary")
def get_summary(start_date: str = None, end_date: str = None, device_id: str = None):
    """Get aggregated summary. If no dates provided, defaults to today."""
    if start_date and end_date:
        start_date_obj = datetime.fromisoformat(start_date).date()
        end_date_obj = datetime.fromisoformat(end_date).date()
        start_ts = datetime(start_date_obj.year, start_date_obj.month, start_date_obj.day).timestamp()
        end_ts = datetime(end_date_obj.year, end_date_obj.month, end_date_obj.day, 23, 59, 59, 999999).timestamp()
    else:
        # Default to today
        today = date.today()
        start_ts = datetime(today.year, today.month, today.day).timestamp()
        end_ts = datetime(today.year, today.month, today.day, 23, 59, 59, 999999).timestamp()

    # Get the data from the DB
    stats = db._get_stats_in_range(start_ts, end_ts, device_id)

    # Calculate total active time merging overlaps
    total_seconds = db.get_total_active_time(start_ts, end_ts, device_id)

    # Format for JSON
    def serialize_usage(u: AppUsage) -> dict:
        data = {
            "name": u.name,
            "identifier_type": u.identifier_type,
            "total_seconds": u.total_seconds,
            "category": u.category,
        }
        if u.children:
            data["children"] = [serialize_usage(c) for c in u.children]
        return data

    serialized_stats = [serialize_usage(s) for s in stats]

    return {
        "start_ts": start_ts,
        "end_ts": end_ts,
        "total_seconds": total_seconds,
        "stats": serialized_stats
    }


@app.get("/api/devices")
def get_devices():
    """Get all unique devices that have reported data."""
    return {"devices": db.get_all_devices()}


@app.get("/api/identifiers")
def get_identifiers():
    """Get all known app and website identifiers for autocomplete."""
    identifiers = db.get_all_app_identifiers()
    return {"identifiers": [{"name": name, "type": type} for name, type in identifiers]}


@app.get("/api/sync/pull")
def pull_sessions(since: float = 0.0):
    """Pull all sessions modified/created after a timestamp."""
    with db._connect() as conn:
        # Note: SQLite doesn't natively track creation time unless we add a column.
        # But we can approximate by pulling sessions where end_time > since.
        # This will fetch all sessions that were active after the last sync.
        rows = conn.execute(
            "SELECT start_time, end_time, app_class, app_title, website_url, device_id FROM sessions WHERE end_time > ? ORDER BY start_time",
            (since,)
        ).fetchall()
        
    sessions = []
    for row in rows:
        sessions.append({
            "start_time": row["start_time"],
            "end_time": row["end_time"],
            "app_class": row["app_class"],
            "app_title": row["app_title"],
            "website_url": row["website_url"],
            "device_id": row["device_id"]
        })
    return {"sessions": sessions}


@app.post("/api/sync/sessions")
def sync_sessions(req: SyncRequest):
    """Sync multiple sessions from a remote device."""
    now = time.time()
    valid_sessions = 0
    for session in req.sessions:
        if session.start_time > session.end_time:
            continue
        if session.end_time > now + 3600: # allow for slight time drift
            continue
        db.insert_session_if_not_exists(
            start_time=session.start_time,
            end_time=session.end_time,
            app_class=session.app_class,
            app_title=session.app_title,
            website_url=session.website_url,
            device_id=req.device_id
        )
        valid_sessions += 1
    return {"status": "success", "synced": valid_sessions}


@app.get("/api/categories")
def get_categories():
    return db._get_category_map()


@app.post("/api/categories")
def set_category(req: CategoryMapRequest):
    cats = {c.name: c.id for c in db.get_categories()}
    cat_id = cats.get(req.category)
    if not cat_id:
        cat_id = db.create_category(req.category)
    db.assign_app_to_category(req.name, req.identifier_type, cat_id)
    return {"status": "success"}


@app.delete("/api/categories")
def delete_category(req: CategoryDeleteRequest):
    db.unassign_app_from_category(req.name)
    return {"status": "success"}


@app.get("/api/groups")
def get_groups():
    return db._get_group_map()


@app.post("/api/groups")
def set_group(req: GroupMapRequest):
    db.add_to_group(group_name=req.group, app_identifier=req.name, identifier_type=req.identifier_type)
    return {"status": "success"}


@app.delete("/api/groups")
def delete_group(req: GroupDeleteRequest):
    db.remove_from_group(req.name)
    return {"status": "success"}


@app.get("/api/rules")
def get_rules():
    rules_dict = db.get_title_rules()
    result = []
    for app_class, targets in rules_dict.items():
        for target in targets:
            result.append({"app_class": app_class, "target_title": target})
    return result


@app.post("/api/rules")
def add_rule(req: RuleRequest):
    db.add_title_rule(req.app_class, req.target_title)
    return {"status": "success"}


@app.delete("/api/rules")
def remove_rule(req: RuleRequest):
    db.remove_title_rule(req.app_class, req.target_title)
    return {"status": "success"}


# Serve React App (Must be at the very bottom so it doesn't override /api routes)
WEB_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), "web", "dist")

@app.get("/")
def read_root():
    return FileResponse(os.path.join(WEB_DIR, "index.html"))

@app.get("/{full_path:path}")
def catch_all(full_path: str):
    # Ignore API routes
    if full_path.startswith("api/"):
        raise HTTPException(status_code=404, detail="API route not found")
        
    file_path = os.path.abspath(os.path.join(WEB_DIR, full_path))
    if not file_path.startswith(os.path.abspath(WEB_DIR)):
        raise HTTPException(status_code=403, detail="Forbidden")

    if os.path.isfile(file_path):
        return FileResponse(file_path)
    return FileResponse(os.path.join(WEB_DIR, "index.html"))

def main():
    """Entry point for the web server."""
    import argparse
    parser = argparse.ArgumentParser(description="ScreenTime Web Server")
    parser.add_argument("--log-console", action="store_true", help="Log to console")
    parser.add_argument("--log-level", type=str, help="Override log level (e.g. DEBUG, INFO)")
    args = parser.parse_args()
    
    from screentime.config import get_config
    from screentime.loggers import setup_uvicorn_logging
    
    config = get_config()
    host = config.get('Server', 'host')
    port = int(config.get('Server', 'port'))
    
    log_config = setup_uvicorn_logging(config, log_console=args.log_console, log_level=args.log_level)
    
    print(f"Starting ScreenTime API Server on http://{host}:{port}")
    uvicorn.run("screentime.api:app", host=host, port=port, log_config=log_config)

if __name__ == "__main__":
    main()
