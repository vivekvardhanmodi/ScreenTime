# Contributing to ScreenTime

Thank you for your interest in contributing to ScreenTime! 

## Project Structure

This overview will help you navigate the codebase:

```text
screentime/
├── screentime/
│   ├── __init__.py           # Constants and paths
│   ├── daemon.py             # Background daemon (asyncio orchestrator)
│   ├── api.py                # FastAPI web server and static file hosting
│   ├── tracker.py            # Hyprland IPC window tracking
│   ├── idle.py               # DPMS screen state tracking (Wayland protocol / IPC polling)
│   ├── database.py           # SQLite operations
│   ├── browser_host.py       # Browser native messaging host
│   └── tui/                  # Main TUI application and views
├── web/                      # React Frontend (Web App)
│   ├── src/                  # React components, pages, CSS
│   └── public/               # Static assets
├── chrome_extension/
│   ├── manifest.json         # Manifest V3
│   ├── background.js         # Service worker (tab tracking)
│   └── icons/
├── firefox_extension/
│   ├── manifest.json         # Manifest V2
│   ├── background.js         # Background script (tab tracking)
│   └── icons/
├── install.sh                # One-command setup
├── set-extension-id.sh       # Chrome extension ID helper
└── pyproject.toml
```
