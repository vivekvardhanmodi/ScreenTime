# ScreenTime

**App usage tracker for Hyprland — like Android's Digital Wellbeing, but for your desktop.**

Track exactly how much time you spend in every app and website. Works entirely offline, stores everything locally, and keeps your data forever.

---

## What It Does

- **Tracks every app** you use — terminals, editors, browsers, everything
- **Tracks websites in Chrome** — YouTube, GitHub, Reddit show up individually, not just "Google Chrome"
- **Ignores idle time** — if you walk away, the clock stops (90s timeout, matching your hypridle config)
- **Stores everything forever** — look up what you were doing on any date, weeks or years from now
- **Groups apps** — combine `foot` + `kitty` into "Terminal", or `youtube.com` + YouTube PWA into "YouTube"
- **Custom categories** — organize apps into "Development", "Entertainment", etc.
- **CSV export** — dump all your data for analysis

---

## Quick Start

### Prerequisites

- **Arch Linux with Hyprland** (Wayland compositor)
- **Python 3.11+** (`sudo pacman -S python`)
- **swayidle** (`sudo pacman -S swayidle`)
- **Google Chrome** (for website tracking)

### Install

```bash
git clone https://github.com/vivekvardhanmodi/screentime.git ~/screentime
cd ~/screentime
chmod +x install.sh
./install.sh
```

The install script will:
1. Create a Python virtual environment
2. Install dependencies (`textual`)
3. Install the Chrome native messaging host

### Set Up Chrome Extension (for website tracking)

1. Open **`chrome://extensions/`** in Google Chrome
2. Enable **Developer mode** (toggle in top right corner)
3. Click **Load unpacked** → select `~/screentime/chrome_extension/`
4. **Copy the Extension ID** shown under the extension name
5. Run this (replace `YOUR_ID` with the actual ID):

```bash
~/screentime/set-extension-id.sh YOUR_ID
```

6. **Restart Chrome**

### Start Tracking

```bash
# Start the daemon in the background
~/screentime/venv/bin/screentimed &

# View your stats
~/screentime/venv/bin/screentime
```

**Auto-start on login:** Add this to your `~/.config/hypr/hyprland.conf`:
```conf
exec-once = ~/screentime/venv/bin/screentimed
```

---

## TUI Usage

Launch the terminal interface:

```bash
~/screentime/venv/bin/screentime
```

### Tabs

| Tab | What it shows |
|-----|--------------|
| **Today** | Live stats for today — total time + per-app breakdown |
| **Daily** | Browse any historical date with ◀ ▶ navigation |
| **Weekly** | Week view with per-day totals + app breakdown |
| **Monthly** | Month view with per-day totals + app breakdown |
| **Groups** | Combine multiple apps into one (e.g., foot + kitty = Terminal) |
| **Categories** | Organize apps into custom categories |
| **Export** | Export all data to CSV |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Tab` | Switch between tabs |
| `r` | Refresh data |
| `q` | Quit |

### How Grouping Works

Groups let you combine multiple apps/websites into a single entry. The original apps are still tracked individually — groups just merge them in the display.

**Example:** You use both `foot` and `kitty` terminals:
```
📦 Terminal                  2h 15m
    └ 🖥  foot               1h 30m
    └ 🖥  kitty              45m
```

Go to the **Groups** tab → click an app → type a group name or select an existing group.

### How Categories Work

Categories are labels you assign to apps for organization (different from groups):
- Groups **merge** time: foot + kitty → "Terminal" (2h 15m)
- Categories **label** apps: Terminal → "Development", YouTube → "Entertainment"

---

## Daemon Management

The daemon (`screentimed`) prevents multiple instances automatically.

```bash
# Start
~/screentime/venv/bin/screentimed &

# Stop
pkill -f screentimed

# View live logs
tail -f ~/.local/share/screentime/daemon.log
```

---

## Important File Locations

| File | Path | Purpose |
|------|------|---------|
| **Database** | `~/.local/share/screentime/screentime.db` | All your usage data, groups, categories |
| **Daemon log** | `~/.local/share/screentime/daemon.log` | Daemon debug log |
| **Chrome URL state** | `$XDG_RUNTIME_DIR/screentime/chrome_url` | Current Chrome tab (ephemeral) |
| **Daemon PID** | `$XDG_RUNTIME_DIR/screentime/daemon.pid` | Running daemon PID (ephemeral) |
| **Chrome native host** | `~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json` | Chrome ↔ daemon bridge config |
| **Chrome host log** | `$XDG_RUNTIME_DIR/screentime/chrome_host.log` | Native messaging host debug log |

### Backup & Migration

Your entire history lives in a single file:

```bash
# Backup
cp ~/.local/share/screentime/screentime.db ~/screentime-backup.db

# Restore on another machine (after running install.sh there)
cp ~/screentime-backup.db ~/.local/share/screentime/screentime.db
```

All groups and categories are stored in the same database — they transfer with it.

---

## Resource Usage

The daemon is designed to be invisible:

| Metric | Value |
|--------|-------|
| **Memory** | ~26 MB (daemon) + ~3 MB (swayidle) |
| **CPU** | ~2 seconds per 35 minutes of runtime |
| **Threads** | 1 (single-threaded async) |
| **Architecture** | Event-driven (no polling) |
| **Database growth** | ~92 KB per day of usage |

---

## Project Structure

```
screentime/
├── screentime/
│   ├── __init__.py           # Constants and paths
│   ├── daemon.py             # Background daemon (asyncio orchestrator)
│   ├── tracker.py            # Hyprland IPC window tracking
│   ├── idle.py               # swayidle idle detection (90s timeout)
│   ├── database.py           # SQLite operations
│   ├── chrome_host.py        # Chrome native messaging host
│   └── tui/
│       ├── app.py            # Main TUI application
│       ├── utils.py          # Formatting utilities
│       ├── dashboard.py      # Today view
│       ├── history.py        # Daily/Weekly/Monthly views
│       ├── categories.py     # Category management
│       ├── groups.py         # Group management
│       └── export.py         # CSV export
├── chrome_extension/
│   ├── manifest.json         # Manifest V3
│   ├── background.js         # Service worker (tab tracking)
│   └── icons/
├── install.sh                # One-command setup
├── set-extension-id.sh       # Chrome extension ID helper
└── pyproject.toml
```

---

## Troubleshooting

### Daemon won't start
```bash
# Check logs
tail -n 30 ~/.local/share/screentime/daemon.log

# Make sure Hyprland is running
echo $HYPRLAND_INSTANCE_SIGNATURE

# Make sure swayidle is installed
which swayidle
```

### Chrome websites not tracking
```bash
# Check if native host is installed
cat ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json

# Check if extension ID is set (should NOT say EXTENSION_ID_PLACEHOLDER)
grep "chrome-extension" ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json

# Check native host logs
cat $XDG_RUNTIME_DIR/screentime/chrome_host.log

# Verify the chrome_host.py is executable
ls -la ~/screentime/screentime/chrome_host.py
```

### TUI shows no data
```bash
# Make sure daemon is running
pgrep -f screentimed

# Check database has data
sqlite3 ~/.local/share/screentime/screentime.db "SELECT COUNT(*) FROM sessions;"
```

### Idle detection not working
```bash
# Test swayidle manually
swayidle -w timeout 10 'echo IDLE' resume 'echo ACTIVE'
# Wait 10 seconds without touching anything — should print IDLE
```

---

## Uninstall

```bash
# Stop the daemon
pkill -f screentime

# Remove Chrome native messaging host
rm ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json

# Remove the Chrome extension from chrome://extensions/

# Remove data (WARNING: deletes all your history!)
rm -rf ~/.local/share/screentime/

# Remove the project
rm -rf ~/screentime/

# Also remove from hyprland.conf if you added it!
```

---

## License

MIT

