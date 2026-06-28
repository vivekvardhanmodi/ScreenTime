import os
import configparser
from pathlib import Path

CONFIG_DIR = Path(os.path.expanduser("~/.config/screentime"))
CONFIG_FILE = CONFIG_DIR / "config.ini"

_config = None

def get_default_data_dir() -> str:
    return os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local" / "share")) + "/screentime"

def _create_default_config() -> configparser.ConfigParser:
    config = configparser.ConfigParser()
    config['Paths'] = {
        'data_dir': get_default_data_dir(),
        'log_dir': get_default_data_dir() + "/logs"
    }
    config['Server'] = {
        'host': '0.0.0.0',
        'port': '8000'
    }
    config['Logging'] = {
        'level': 'INFO',
        'max_size_mb': '10',
        'backup_count': '5',
        'console': 'false'
    }
    config['Daemon'] = {
        'idle_timeout': '90',
        'heartbeat_interval': '10',
        'chrome_window_class': 'google-chrome',
        'firefox_window_class': 'firefox'
    }
    return config

def load_config(path: Path = None) -> configparser.ConfigParser:
    config = _create_default_config()
    cfg_file = path or CONFIG_FILE
    if cfg_file.exists():
        config.read(cfg_file)
    else:
        cfg_file.parent.mkdir(parents=True, exist_ok=True)
        with open(cfg_file, 'w') as f:
            config.write(f)
    return config

def get_config() -> configparser.ConfigParser:
    global _config
    if _config is None:
        _config = load_config()
    return _config
