#!/usr/bin/env bash
#
# ScreenTime Installation Script
# Sets up venv, dependencies, and Chrome native messaging host.
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VENV_DIR="$SCRIPT_DIR/venv"
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

# ── Set up native messaging host ──────────────────────────────────────

info "Setting up native messaging host..."

chmod +x "$SCRIPT_DIR/screentime/browser_host.py"

# ── Chrome ──
CHROME_NMH_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
mkdir -p "$CHROME_NMH_DIR"

cat > "$CHROME_NMH_DIR/$NATIVE_HOST_NAME.json" << EOF
{
  "name": "$NATIVE_HOST_NAME",
  "description": "ScreenTime browser URL tracker",
  "path": "$SCRIPT_DIR/screentime/browser_host.py",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://EXTENSION_ID_PLACEHOLDER/"
  ]
}
EOF

ok "Chrome native messaging host installed"

# ── Firefox ──
FIREFOX_NMH_DIR="$HOME/.mozilla/native-messaging-hosts"
mkdir -p "$FIREFOX_NMH_DIR"

cat > "$FIREFOX_NMH_DIR/$NATIVE_HOST_NAME.json" << EOF
{
  "name": "$NATIVE_HOST_NAME",
  "description": "ScreenTime browser URL tracker",
  "path": "$SCRIPT_DIR/screentime/browser_host.py",
  "type": "stdio",
  "allowed_extensions": [
    "screentime@screentime.local"
  ]
}
EOF

ok "Firefox native messaging host installed"

# ── Create symlinks in ~/.local/bin ──────────────────────────────────

info "Creating symlinks in ~/.local/bin..."
mkdir -p "$HOME/.local/bin"
ln -sf "$VENV_DIR/bin/screentime" "$HOME/.local/bin/screentime"
ln -sf "$VENV_DIR/bin/screentimed" "$HOME/.local/bin/screentimed"
ln -sf "$VENV_DIR/bin/screentime-web" "$HOME/.local/bin/screentime-web"
ok "Symlinks created in ~/.local/bin"

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
echo -e "     ${GREEN}screentimed &${NC} (or $VENV_DIR/bin/screentimed &)"
echo ""
echo -e "  ${YELLOW}4. Launch the TUI:${NC}"
echo -e "     ${GREEN}screentime${NC} (or $VENV_DIR/bin/screentime)"
echo ""
echo -e "  ${YELLOW}5. Launch the Web App:${NC}"
echo -e "     ${GREEN}screentime-web${NC} (or $VENV_DIR/bin/screentime-web)"
echo ""
echo -e "  ${YELLOW}6. Auto-start on login:${NC}"
echo "     Add this to your hyprland.conf:"
echo -e "     ${GREEN}exec-once = screentimed${NC} (or exec-once = $VENV_DIR/bin/screentimed)"
echo ""
echo -e "${BLUE}Useful commands:${NC}"
echo "  Logs:  tail -f $DATA_DIR/daemon.log"
echo "  Stop:  pkill -f screentimed"
echo ""
