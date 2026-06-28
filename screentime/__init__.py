"""ScreenTime — Linux app usage tracker for Hyprland."""

__version__ = "0.1.0"

import os
from pathlib import Path

def get_data_dir() -> Path:
    from screentime.config import get_config
    return Path(os.path.expanduser(get_config().get('Paths', 'data_dir')))

def get_runtime_dir() -> Path:
    return Path(os.environ.get("XDG_RUNTIME_DIR", f"/tmp/screentime-{os.getuid()}")) / "screentime"

def init_env():
    """Create required directories."""
    get_data_dir().mkdir(parents=True, exist_ok=True)
    get_runtime_dir().mkdir(parents=True, exist_ok=True)

def get_db_path() -> Path:
    return get_data_dir() / "screentime.db"

def get_chrome_url_file() -> Path:
    return get_runtime_dir() / "chrome_url"

def get_chrome_window_class() -> str:
    from screentime.config import get_config
    return get_config().get('Daemon', 'chrome_window_class')

def get_firefox_window_class() -> str:
    from screentime.config import get_config
    return get_config().get('Daemon', 'firefox_window_class')

def get_idle_timeout() -> int:
    from screentime.config import get_config
    return int(get_config().get('Daemon', 'idle_timeout'))

def get_heartbeat_interval() -> int:
    from screentime.config import get_config
    return int(get_config().get('Daemon', 'heartbeat_interval'))
