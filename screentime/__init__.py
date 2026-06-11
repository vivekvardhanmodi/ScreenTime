"""ScreenTime — Linux app usage tracker for Hyprland."""

__version__ = "0.1.0"

import os
from pathlib import Path

# Data directory for database and state files
DATA_DIR = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share")) / "screentime"
DATA_DIR.mkdir(parents=True, exist_ok=True)

# Runtime directory for ephemeral state (chrome URL, PID file)
RUNTIME_DIR = Path(os.environ.get("XDG_RUNTIME_DIR", f"/tmp/screentime-{os.getuid()}")) / "screentime"
RUNTIME_DIR.mkdir(parents=True, exist_ok=True)

# Database path
DB_PATH = DATA_DIR / "screentime.db"

# Chrome URL state file — written by native messaging host, read by daemon
CHROME_URL_FILE = RUNTIME_DIR / "chrome_url"

# The Hyprland window class for Google Chrome
CHROME_WINDOW_CLASS = "google-chrome"

# The Hyprland window class for Firefox
FIREFOX_WINDOW_CLASS = "firefox"

# Idle timeout in seconds (matches hypridle.conf)
IDLE_TIMEOUT = 90

# Heartbeat interval — how often we update the current session's end_time
HEARTBEAT_INTERVAL = 10
