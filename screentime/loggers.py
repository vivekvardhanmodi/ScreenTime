import logging
from logging.handlers import RotatingFileHandler
from pathlib import Path
import configparser
import os
import uvicorn

def setup_logging(config: configparser.ConfigParser, log_filename: str, log_console: bool = None, log_level: str = None):
    """Centralized logging setup for non-uvicorn applications."""
    log_dir = Path(os.path.expanduser(config.get('Paths', 'log_dir')))
    log_dir.mkdir(parents=True, exist_ok=True)
    
    log_file = log_dir / log_filename
    max_bytes = int(config.get('Logging', 'max_size_mb')) * 1024 * 1024
    backup_count = int(config.get('Logging', 'backup_count'))
    
    if log_console is None:
        log_console = config.getboolean('Logging', 'console', fallback=False)
        
    level_str = log_level if log_level else config.get('Logging', 'level').upper()
    level = getattr(logging, level_str, logging.INFO)
    
    root_logger = logging.getLogger()
    root_logger.setLevel(level)
    
    if root_logger.hasHandlers():
        root_logger.handlers.clear()
        
    formatter = logging.Formatter('%(asctime)s [%(levelname)s] %(name)s - %(message)s')
    
    file_handler = RotatingFileHandler(
        log_file,
        maxBytes=max_bytes,
        backupCount=backup_count,
        encoding="utf-8"
    )
    file_handler.setFormatter(formatter)
    root_logger.addHandler(file_handler)
    
    if log_console:
        console_handler = logging.StreamHandler()
        console_handler.setFormatter(formatter)
        root_logger.addHandler(console_handler)

def setup_uvicorn_logging(config: configparser.ConfigParser, log_console: bool = None, log_level: str = None) -> dict:
    """Creates a logging configuration dictionary for uvicorn.run()."""
    log_dir = Path(os.path.expanduser(config.get('Paths', 'log_dir')))
    log_dir.mkdir(parents=True, exist_ok=True)
    
    log_file = log_dir / "web.log"
    max_bytes = int(config.get('Logging', 'max_size_mb')) * 1024 * 1024
    backup_count = int(config.get('Logging', 'backup_count'))
    
    if log_console is None:
        log_console = config.getboolean('Logging', 'console', fallback=True) # web server usually prints to console by default
        
    level_str = log_level if log_level else config.get('Logging', 'level').upper()
    
    log_config = uvicorn.config.LOGGING_CONFIG.copy()
    
    # Add timestamps to uvicorn's formatters
    if "%(asctime)s" not in log_config["formatters"]["default"].get("fmt", ""):
        log_config["formatters"]["default"]["fmt"] = "%(asctime)s " + log_config["formatters"]["default"].get("fmt", "%(levelprefix)s %(message)s")
    if "%(asctime)s" not in log_config["formatters"]["access"].get("fmt", ""):
        log_config["formatters"]["access"]["fmt"] = "%(asctime)s " + log_config["formatters"]["access"].get("fmt", "%(levelprefix)s %(client_addr)s - \"%(request_line)s\" %(status_code)s")
    
    # Custom file handler
    log_config["handlers"]["file"] = {
        "class": "logging.handlers.RotatingFileHandler",
        "filename": str(log_file),
        "maxBytes": max_bytes,
        "backupCount": backup_count,
        "encoding": "utf-8",
        "formatter": "default"
    }
    
    handlers = ["file"]
    if log_console:
        handlers.append("default") # Uvicorn's default console handler
        
    # Overwrite handlers for uvicorn loggers
    log_config["loggers"]["uvicorn"]["handlers"] = handlers
    log_config["loggers"]["uvicorn.error"]["handlers"] = handlers
    
    # Access logs usually use the access formatter
    access_handlers = ["file"]
    if log_console:
        access_handlers.append("access")
    log_config["loggers"]["uvicorn.access"]["handlers"] = access_handlers
    
    # Set levels
    log_config["loggers"]["uvicorn"]["level"] = level_str
    log_config["loggers"]["uvicorn.error"]["level"] = level_str
    log_config["loggers"]["uvicorn.access"]["level"] = level_str
    
    # Prevent duplicate logs from propagation
    log_config["loggers"]["uvicorn"]["propagate"] = False
    log_config["loggers"]["uvicorn.error"]["propagate"] = False
    log_config["loggers"]["uvicorn.access"]["propagate"] = False
    
    return log_config
