# ScreenTime

**App usage tracker for Hyprland — like Android's Digital Wellbeing, but for your desktop.**

Track exactly how much time you spend in every app and website. Works entirely offline, stores everything locally, and keeps your data forever.

---

## What It Does

- **Tracks every app** you use — terminals, editors, browsers, everything
- **Tracks websites in Chrome & Firefox** — YouTube, GitHub, Reddit show up individually, not just "Google Chrome"
- **Ignores idle time** — if you walk away, the clock stops by detecting screen power state (DPMS). Leverages your native idle manager.
- **Stores everything forever** — look up what you were doing on any date, weeks or years from now
- **Groups apps** — combine `foot` + `kitty` into "Terminal", or `youtube.com` + YouTube PWA into "YouTube"
- **Custom categories** — organize apps into "Development", "Entertainment", etc.
- **Title rules** — split app windows into separate tracking entities based on window title prefixes
- **CSV export** — dump all your data for analysis

---

## Quick Start

### Prerequisites

- **Arch Linux with Hyprland** (Wayland compositor)
- **Python 3.11+** (`sudo pacman -S python`)
- **Node.js (v18+) & npm** (`sudo pacman -S nodejs npm`) — for building the Web App
- **Google Chrome** and/or **Firefox** (for website tracking)

### Install

```bash
git clone https://github.com/vivekvardhanmodi/screentime.git ~/screentime
cd ~/screentime
chmod +x install.sh
./install.sh
```

The install script will:
1. Create a Python virtual environment
2. Install Python dependencies
3. Build the React Web App frontend (`web/dist`)
4. Install native messaging hosts for Chrome and Firefox

### Set Up Browser Extension (for website tracking)

#### Chrome

1. Open **`chrome://extensions/`**
2. Enable **Developer mode** (top right)
3. Click **Load unpacked** → select `~/screentime/chrome_extension/`
4. **Copy the Extension ID** shown under the extension name
5. Run this (replace `YOUR_ID` with the actual ID):

```bash
~/screentime/set-extension-id.sh YOUR_ID
```

6. **Restart Chrome**

#### Firefox

1. Open **`about:debugging#/runtime/this-firefox`**
2. Click **Load Temporary Add-on**
3. Select `~/screentime/firefox_extension/manifest.json`

No extra ID step needed — Firefox uses a fixed extension ID.

### Start Tracking

```bash
# Start the daemon in the background
screentimed &

# Open the Web App
screentime-web
# (Navigate to http://localhost:8000 in your browser)

# Or, use the TUI (Terminal Interface)
screentime
```

**Auto-start on login:** Add this to your `~/.config/hypr/hyprland.conf`:
```conf
exec-once = screentimed
```

---

## Web App & TUI Usage

ScreenTime provides a beautiful, modern **Web Application** and a lightweight **Terminal UI**.

### Launch the Web App

```bash
screentime-web
```
Then navigate to `http://localhost:8000` in your browser.

### Launch the TUI

```bash
screentime
```

### Views & Features

| View | Web App | TUI | Description |
|------|---------|-----|-------------|
| **Dashboard / Today** | ✅ | ✅ | Live stats for today — total time, category breakdown, top usage |
| **History / Daily / Weekly / Monthly** | ✅ | ✅ | Browse historical dates and custom date ranges |
| **Categories** | ✅ | ✅ | Organize apps into custom categories (Development, Entertainment, etc.) |
| **Groups** | ✅ | ✅ | Combine multiple apps into one (e.g., foot + kitty = Terminal) |
| **Title Rules** | ✅ | ✅ | Create rules to split app tracking based on window titles |
| **Export** | — | ✅ | Export all data to CSV |

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
screentimed &

# Stop
pkill -f screentimed

# View live logs
tail -f ~/.local/share/screentime/logs/daemon.log
```

---

## Important File Locations

| File | Path | Purpose |
|------|------|---------|
| **Database** | `~/.local/share/screentime/screentime.db` | All your usage data, groups, categories |
| **Daemon log** | `~/.local/share/screentime/logs/daemon.log` | Daemon debug log |
| **Chrome URL state** | `$XDG_RUNTIME_DIR/screentime/chrome_url` | Current Chrome tab (ephemeral) |
| **Firefox URL state** | `$XDG_RUNTIME_DIR/screentime/firefox_url` | Current Firefox tab (ephemeral) |
| **Daemon PID** | `$XDG_RUNTIME_DIR/screentime/daemon.pid` | Running daemon PID (ephemeral) |
| **Chrome native host** | `~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json` | Chrome ↔ daemon bridge config |
| **Firefox native host** | `~/.mozilla/native-messaging-hosts/com.screentime.native.json` | Firefox ↔ daemon bridge config |
| **Native host log** | `$XDG_RUNTIME_DIR/screentime/chrome_host.log` | Native messaging host debug log |

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
| **Memory** | ~26 MB (daemon) |
| **CPU** | ~2 seconds per 35 minutes of runtime |
| **Threads** | 1 (single-threaded async) |
| **Architecture** | Event-driven (no polling) |
| **Database growth** | ~92 KB per day of usage |

---

## Troubleshooting

### Daemon won't start
```bash
# Check logs
tail -n 30 ~/.local/share/screentime/logs/daemon.log

# Make sure Hyprland is running
echo $HYPRLAND_INSTANCE_SIGNATURE
```

### Browser websites not tracking
```bash
# Check if native host is installed (Chrome)
cat ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json

# Check if native host is installed (Firefox)
cat ~/.mozilla/native-messaging-hosts/com.screentime.native.json

# Check if Chrome extension ID is set (should NOT say EXTENSION_ID_PLACEHOLDER)
grep "chrome-extension" ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json

# Check native host logs
cat $XDG_RUNTIME_DIR/screentime/chrome_host.log

# Verify the browser_host.py is executable
ls -la ~/screentime/screentime/browser_host.py
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
# Check if your screen turns off when idle (handled by your native idle manager).
# The tracker listens for Wayland DPMS state changes or polls Hyprland monitor status.
```

---

## Uninstall

```bash
# Stop the daemon
pkill -f screentime

# Remove native messaging hosts
rm ~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json
rm ~/.mozilla/native-messaging-hosts/com.screentime.native.json

# Remove browser extensions (chrome://extensions/ and about:addons)

# Remove data (WARNING: deletes all your history!)
rm -rf ~/.local/share/screentime/

# Remove the project
rm -rf ~/screentime/

# Also remove from hyprland.conf if you added it!
```

---

## License

MIT

