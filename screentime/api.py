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

from screentime.database import Database, AppUsage

app = FastAPI(title="ScreenTime API")

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

@app.on_event("startup")
def startup_event():
    global db
    db = Database()

# --- Models ---

class CategoryMapRequest(BaseModel):
    name: str
    category: str

class CategoryDeleteRequest(BaseModel):
    name: str

class GroupMapRequest(BaseModel):
    name: str
    group: str

class GroupDeleteRequest(BaseModel):
    name: str

class RuleRequest(BaseModel):
    app_class: str
    target_title: str

# --- Endpoints ---

@app.get("/api/summary")
def get_summary(start_date: str = None, end_date: str = None):
    """Get aggregated summary. If no dates provided, defaults to today."""
    if start_date and end_date:
        start_ts = datetime.fromisoformat(start_date).timestamp()
        end_ts = datetime.fromisoformat(end_date).timestamp() + 86400  # include the whole end day
    else:
        # Default to today
        today = date.today()
        start_ts = datetime(today.year, today.month, today.day).timestamp()
        end_ts = time.time()

    # Get the data from the DB
    stats = db._get_stats_in_range(start_ts, end_ts)

    # Calculate total active time
    total_seconds = sum(item.total_seconds for item in stats)

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


@app.get("/api/categories")
def get_categories():
    return db._get_category_map()


@app.post("/api/categories")
def set_category(req: CategoryMapRequest):
    db.set_category(req.name, req.category)
    return {"status": "success"}


@app.delete("/api/categories")
def delete_category(req: CategoryDeleteRequest):
    db.remove_category(req.name)
    return {"status": "success"}


@app.get("/api/groups")
def get_groups():
    return db._get_group_map()


@app.post("/api/groups")
def set_group(req: GroupMapRequest):
    db.set_group(req.name, req.group)
    return {"status": "success"}


@app.delete("/api/groups")
def delete_group(req: GroupDeleteRequest):
    db.remove_group(req.name)
    return {"status": "success"}


@app.get("/api/rules")
def get_rules():
    return db.get_title_rules()


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
        
    file_path = os.path.join(WEB_DIR, full_path)
    if os.path.isfile(file_path):
        return FileResponse(file_path)
    return FileResponse(os.path.join(WEB_DIR, "index.html"))

def main():
    """Entry point for the web server."""
    print("Starting ScreenTime API Server on http://127.0.0.1:8000")
    uvicorn.run("screentime.api:app", host="127.0.0.1", port=8000, reload=True)

if __name__ == "__main__":
    main()
