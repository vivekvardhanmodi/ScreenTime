# ScreenTime

ScreenTime is an application and website usage tracker for Linux and Windows.
It measures time spent in desktop applications and active browser tabs, stores all data in a local SQLite database, and runs without external network requests.

## Features

- Records active application windows on Wayland, X11, and Windows.
- Tracks active website domains in Google Chrome and Mozilla Firefox.
- Halts tracking during user inactivity, monitor power-down (DPMS), and system suspend.
- Runs as an asynchronous native Rust daemon consuming under 10 MB of memory.
- Combines related applications and websites into custom groups.
- Organizes tracked items into categories such as Development or Communication.
- Splits application tracking based on window title rules.
- Exports stored records to CSV files.

## Prerequisites

- **Operating system.** Linux with Hyprland, Sway, KDE Plasma, or an X11 window manager, or Windows 10 or 11.
- **Rust toolchain.** Cargo and rustc version 1.78 or newer.
- **Node.js and npm.** Node.js version 18 or newer to compile the web interface.
- **Python.** Python 3.11 or newer to run the web dashboard server and terminal UI.
- **Web browser.** Google Chrome, Chromium, or Mozilla Firefox for domain tracking.

## Installation

Clone the repository and run the automated installer:

```bash
git clone https://github.com/vivekvardhanmodi/screentime.git ~/screentime
cd ~/screentime
chmod +x install.sh
./install.sh
```

The installation script performs the following tasks:
1. Compiles the native Rust binaries (`screentimed` and `screentime-browser-host`) using `cargo build --release`.
2. Installs the compiled binaries into `~/.local/bin/`.
3. Creates a Python virtual environment and installs backend dependencies.
4. Compiles the React web dashboard into `web/dist`.
5. Installs browser native messaging manifests for Chrome and Firefox.

## Browser extension setup

To track individual website domains, install the extension in your browser.

### Google Chrome and Chromium

1. Open `chrome://extensions/` in your browser.
2. Turn on the **Developer mode** toggle in the top right corner.
3. Select **Load unpacked** and choose `~/screentime/chrome_extension/`.
4. Copy the extension ID displayed under the ScreenTime entry.
5. Register the ID with the native messaging host:

```bash
~/screentime/set-extension-id.sh YOUR_EXTENSION_ID
```

6. Restart the browser.

### Mozilla Firefox

1. Open `about:debugging#/runtime/this-firefox` in Firefox.
2. Select **Load Temporary Add-on**.
3. Select `~/screentime/firefox_extension/manifest.json`.

Firefox uses a fixed extension ID, so no extra configuration script is required.

## Daemon management

The daemon manages window monitoring and database persistence.
It prevents multiple instances by holding an exclusive lock on its IPC socket.

### Running the daemon

Run the daemon directly from your shell:

```bash
# Start daemon in the background
screentimed &

# Inspect running daemon status over IPC
screentimed status

# Stop running daemon instances
pkill -f screentimed
```

### Autostart configuration

The installer does not configure autostart services.
Configure autostart through your compositor or create an optional systemd user service.

#### Compositor autostart

Add `screentimed` to your compositor startup commands.

For Hyprland, add this line to `~/.config/hypr/hyprland.conf`:

```ini
exec-once = screentimed
```

For Sway, add this line to `~/.config/sway/config`:

```ini
exec screentimed
```

#### Optional systemd user service

To manage the daemon as a systemd user unit, create `~/.config/systemd/user/screentimed.service`:

```ini
[Unit]
Description=ScreenTime Tracking Daemon
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/screentimed
Restart=on-failure

[Install]
WantedBy=default.target
```

Reload the user daemon and start the service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now screentimed
```

Manage the running unit with user commands:

```bash
# Check service status
systemctl --user status screentimed

# Stop the service
systemctl --user stop screentimed

# View live systemd logs
journalctl --user -u screentimed -f
```

### CLI options

```text
screentimed [SUBCOMMAND] [OPTIONS]

SUBCOMMANDS:
    (default)            Run the background tracking daemon
    status               Query running daemon status over IPC
    install-manifests    Install Chrome and Firefox native messaging manifests
    help                 Print help information
    version              Print version information

OPTIONS:
    --platform <NAME>    Force platform backend (hyprland, kde, wlroots, x11, windows)
    --db <PATH>          Path to SQLite database file
    --socket <PATH>      Path to IPC domain socket or Windows named pipe
    --config <PATH>      Path to custom config.ini configuration file
    --idle-timeout <SEC> Inactivity timeout in seconds before pausing tracking
    --log-console        Output structured logs to stderr
    --log-level <LEVEL>  Log verbosity (trace, debug, info, warn, error)
```

## Frontends

ScreenTime provides a web interface and a terminal interface.

### Web application

Launch the local web server:

```bash
screentime-web
```

Open `http://localhost:8000` in your web browser.

### Terminal UI

Launch the terminal dashboard:

```bash
screentime
```

Use `Tab` to navigate between views, `r` to refresh, and `q` to exit.

## File locations

| Resource | Path | Description |
| :--- | :--- | :--- |
| SQLite database | `~/.local/share/screentime/screentime.db` | All tracked sessions, categories, and groups. |
| Daemon log file | `~/.local/share/screentime/logs/screentimed.log` | Rolling daemon diagnostic log. |
| Daemon IPC socket | `$XDG_RUNTIME_DIR/screentime.sock` | Local Unix domain socket on Linux. |
| Daemon named pipe | `\\.\pipe\screentime` | Local named pipe on Windows. |
| Chrome native host | `~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json` | Chrome messaging registration. |
| Firefox native host | `~/.mozilla/native-messaging-hosts/com.screentime.native.json` | Firefox messaging registration. |

## Platform support

ScreenTime detects your desktop environment on startup.
Detailed platform architecture is documented in [docs/architecture.md](docs/architecture.md).

| Platform | Window manager | Protocol mechanism | Status |
| :--- | :--- | :--- | :--- |
| Linux | KDE Plasma 6 | KWin Scripting D-Bus bridge | Supported |
| Linux | Sway, River, Niri, Wayfire | wlroots foreign toplevel v1 | Supported |
| Linux | Openbox, i3, bspwm, XFCE | X11 EWMH root window properties | Supported |
| Linux | Hyprland | Dedicated Hyprland Unix socket | Supported |
| Windows | Windows 10 / 11 | Win32 hooks, adaptive polling, DWM | Supported |
| Linux | GNOME Wayland | Mutter compositor | Not supported |

## Database backups

To back up your tracking history, copy the SQLite database file:

```bash
cp ~/.local/share/screentime/screentime.db ~/screentime-backup.db
```

To restore data, copy the backup file back to `~/.local/share/screentime/screentime.db`.

## License

MIT
