import configparser
import os
import tempfile
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
        'log_dir': get_default_data_dir() + "/logs",
    }
    config['Server'] = {
        'host': '0.0.0.0',
        'port': '8000',
    }
    config['Logging'] = {
        'level': 'INFO',
        'max_size_mb': '10',
        'backup_count': '5',
        'console': 'false',
    }
    config['Daemon'] = {
        'idle_timeout': '90',
        'heartbeat_interval': '10',
        'chrome_window_class': 'google-chrome',
        'firefox_window_class': 'firefox',
    }
    config['TUI'] = {
        'theme': 'textual-dark',
    }
    return config


def save_config(config: configparser.ConfigParser, path: Path | None = None) -> None:
    cfg_file = Path(path) if path is not None else CONFIG_FILE
    cfg_file.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode='w',
        dir=cfg_file.parent,
        prefix=f"{cfg_file.name}.",
        suffix=".tmp",
        delete=False,
    ) as f:
        temp_path = Path(f.name)
        config.write(f)
    os.replace(temp_path, cfg_file)


def load_config(path: Path | None = None) -> configparser.ConfigParser:
    config = _create_default_config()
    cfg_file = Path(path) if path is not None else CONFIG_FILE
    if cfg_file.exists():
        config.read(cfg_file)
    else:
        save_config(config, cfg_file)
    return config


def get_config() -> configparser.ConfigParser:
    global _config
    if _config is None:
        _config = load_config()
    return _config


def get_theme(path: Path | None = None) -> str:
    cfg_file = Path(path) if path is not None else CONFIG_FILE
    if not cfg_file.exists():
        return 'textual-dark'
    parser = configparser.ConfigParser()
    parser.read(cfg_file)
    return parser.get('TUI', 'theme', fallback='textual-dark').strip() or 'textual-dark'


def save_theme(theme: str, path: Path | None = None) -> None:
    global _config
    cfg_file = Path(path) if path is not None else CONFIG_FILE
    parser = configparser.ConfigParser()
    if cfg_file.exists():
        parser.read(cfg_file)
    if not parser.has_section('TUI'):
        parser.add_section('TUI')
    parser.set('TUI', 'theme', theme)
    save_config(parser, cfg_file)
    if _config is not None and (path is None or cfg_file == CONFIG_FILE):
        if not _config.has_section('TUI'):
            _config.add_section('TUI')
        _config.set('TUI', 'theme', theme)
