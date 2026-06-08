#!/usr/bin/env bash
#
# ScreenTime Installation Script
# Sets up venv, dependencies, systemd service, and Chrome native messaging host.
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VENV_DIR="$SCRIPT_DIR/venv"
SERVICE_NAME="screentime-daemon"
NATIVE_HOST_NAME="com.screentime.native"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

echo -e "${PURPLE}"
echo "  ╔═══════════════════════════════════╗"
echo "  ║    ScreenTime Installer           ║"
echo "  ║    App Usage Tracker for Linux    ║"
echo "  ╚═══════════════════════════════════╝"
echo -e "${NC}"

# ── Check prerequisites ──────────────────────────────────────────────

info "Checking prerequisites..."

if ! command -v python3 &>/dev/null; then
    error "python3 not found. Install it: sudo pacman -S python"
    exit 1
fi

if ! command -v hyprctl &>/dev/null; then
    warn "hyprctl not found — Hyprland may not be running or installed."
fi

if ! command -v swayidle &>/dev/null; then
    warn "swayidle not found. Installing..."
    echo -e "  ${YELLOW}Run: sudo pacman -S swayidle${NC}"
    echo -n "  Install now? [Y/n] "
    read -r answer
    if [[ "$answer" != "n" && "$answer" != "N" ]]; then
        sudo pacman -S --noconfirm swayidle
    fi
fi

ok "Prerequisites checked"

# ── Create virtual environment ────────────────────────────────────────

info "Setting up Python virtual environment..."

if [[ ! -d "$VENV_DIR" ]]; then
    python3 -m venv "$VENV_DIR"
    ok "Created venv at $VENV_DIR"
else
    ok "Venv already exists at $VENV_DIR"
fi

source "$VENV_DIR/bin/activate"

info "Installing Python dependencies..."
pip install --upgrade pip -q
pip install -e "$SCRIPT_DIR" -q
ok "Dependencies installed"

# ── Create data directory ─────────────────────────────────────────────

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/screentime"
mkdir -p "$DATA_DIR"
ok "Data directory: $DATA_DIR"

# ── Install systemd service ───────────────────────────────────────────

info "Installing systemd user service..."

SYSTEMD_DIR="$HOME/.config/systemd/user"
mkdir -p "$SYSTEMD_DIR"

# Generate service file with absolute paths
cat > "$SYSTEMD_DIR/$SERVICE_NAME.service" << EOF
[Unit]
Description=ScreenTime Activity Tracker Daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
Environment=PYTHONPATH=$SCRIPT_DIR
ExecStart=$VENV_DIR/bin/python -m screentime.daemon
Restart=on-failure
RestartSec=5
WorkingDirectory=$SCRIPT_DIR

[Install]
WantedBy=graphical-session.target
EOF

systemctl --user daemon-reload
systemctl --user enable "$SERVICE_NAME.service"
ok "systemd service installed and enabled"

# ── Make Chrome native messaging host executable ──────────────────────

info "Setting up Chrome native messaging host..."

chmod +x "$SCRIPT_DIR/screentime/chrome_host.py"

# Install native messaging host manifest for Google Chrome
CHROME_NMH_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
mkdir -p "$CHROME_NMH_DIR"

cat > "$CHROME_NMH_DIR/$NATIVE_HOST_NAME.json" << EOF
{
  "name": "$NATIVE_HOST_NAME",
  "description": "ScreenTime Chrome URL tracker",
  "path": "$SCRIPT_DIR/screentime/chrome_host.py",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://EXTENSION_ID_PLACEHOLDER/"
  ]
}
EOF

ok "Native messaging host manifest installed"

# ── Print next steps ──────────────────────────────────────────────────

echo ""
echo -e "${PURPLE}═══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}Installation complete!${NC}"
echo -e "${PURPLE}═══════════════════════════════════════════════════${NC}"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo ""
echo -e "  ${YELLOW}1. Load the Chrome extension:${NC}"
echo "     • Open chrome://extensions/ in Google Chrome"
echo "     • Enable 'Developer mode' (top right)"
echo "     • Click 'Load unpacked'"
echo "     • Select: $SCRIPT_DIR/chrome_extension/"
echo "     • Copy the extension ID shown"
echo ""
echo -e "  ${YELLOW}2. Update the native messaging host with your extension ID:${NC}"
echo "     Run this command (replace YOUR_EXTENSION_ID):"
echo ""
echo -e "     ${GREEN}sed -i 's/EXTENSION_ID_PLACEHOLDER/YOUR_EXTENSION_ID/' \\"
echo -e "       $CHROME_NMH_DIR/$NATIVE_HOST_NAME.json${NC}"
echo ""
echo -e "  ${YELLOW}3. Start the daemon:${NC}"
echo -e "     ${GREEN}systemctl --user start $SERVICE_NAME${NC}"
echo ""
echo -e "  ${YELLOW}4. Launch the TUI:${NC}"
echo -e "     ${GREEN}$VENV_DIR/bin/screentime${NC}"
echo ""
echo -e "  ${YELLOW}5. Auto-start on login (already enabled):${NC}"
echo "     The daemon will start automatically on next login."
echo "     Or add to hyprland.conf:"
echo -e "     ${GREEN}exec-once = systemctl --user start $SERVICE_NAME${NC}"
echo ""
echo -e "${BLUE}Useful commands:${NC}"
echo "  Status:  systemctl --user status $SERVICE_NAME"
echo "  Logs:    journalctl --user -u $SERVICE_NAME -f"
echo "  Stop:    systemctl --user stop $SERVICE_NAME"
echo "  Restart: systemctl --user restart $SERVICE_NAME"
echo ""
