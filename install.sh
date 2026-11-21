#!/usr/bin/env bash
#
# ScreenTime Installation Script
# Sets up Rust release binaries, Python virtual environment, and browser native messaging hosts.
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VENV_DIR="$SCRIPT_DIR/venv"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

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

# ── Check System Prerequisites ────────────────────────────────────────

info "Checking system prerequisites..."

if ! command -v cargo &>/dev/null; then
    error "cargo is not installed. Install Rust: https://rustup.rs/ or via your package manager."
    exit 1
fi

if ! command -v python3 &>/dev/null; then
    error "python3 is not installed. Install it via your package manager (e.g. sudo pacman -S python)."
    exit 1
fi

if ! command -v hyprctl &>/dev/null; then
    warn "hyprctl not found — Hyprland may not be running or installed."
fi

if ! command -v npm &>/dev/null; then
    warn "npm not found. Node.js & npm are recommended to build the Web App (e.g. sudo pacman -S nodejs npm)."
fi

ok "Prerequisites checked successfully."

# ── Build Rust Release Binaries ───────────────────────────────────────

info "Building Rust release binaries (cargo build --release)..."
(cd "$SCRIPT_DIR" && cargo build --release)

if [[ ! -f "$SCRIPT_DIR/target/release/screentimed" || ! -f "$SCRIPT_DIR/target/release/screentime-browser-host" ]]; then
    error "Rust binaries were not found in target/release after build."
    exit 1
fi
ok "Rust release binaries built successfully."

# ── Install Rust Binaries to ~/.local/bin ─────────────────────────────

info "Installing Rust binaries to $BIN_DIR..."
mkdir -p "$BIN_DIR"
ln -sf "$SCRIPT_DIR/target/release/screentimed" "$BIN_DIR/screentimed"
ln -sf "$SCRIPT_DIR/target/release/screentime-browser-host" "$BIN_DIR/screentime-browser-host"
ok "Rust binaries installed in $BIN_DIR"

# ── Set Up Native Messaging Host Manifests ────────────────────────────

info "Configuring browser native messaging manifests..."
MANIFEST_ARGS=(--host-path "$BIN_DIR/screentime-browser-host")
if [[ -n "${CHROME_NMH_DIR:-}" ]]; then
    MANIFEST_ARGS+=(--chrome-dir "$CHROME_NMH_DIR")
fi
if [[ -n "${FIREFOX_NMH_DIR:-}" ]]; then
    MANIFEST_ARGS+=(--firefox-dir "$FIREFOX_NMH_DIR")
fi
"$BIN_DIR/screentimed" install-manifests "${MANIFEST_ARGS[@]}"
ok "Browser native messaging manifests configured."

# ── Setup Python Virtual Environment ──────────────────────────────────

info "Setting up Python virtual environment..."

if [[ ! -d "$VENV_DIR" ]]; then
    python3 -m venv "$VENV_DIR"
    ok "Created venv at $VENV_DIR"
else
    ok "Venv already exists at $VENV_DIR"
fi

source "$VENV_DIR/bin/activate"

info "Installing Python dependencies for TUI and Web API..."
pip install --upgrade pip -q
pip install -e "$SCRIPT_DIR" -q
rm -f "$VENV_DIR/bin/screentimed"
ok "Python dependencies installed."

info "Installing Python CLI symlinks to $BIN_DIR..."
ln -sf "$VENV_DIR/bin/screentime" "$BIN_DIR/screentime"
ln -sf "$VENV_DIR/bin/screentime-web" "$BIN_DIR/screentime-web"
ok "Python CLI symlinks created in $BIN_DIR"

# ── Build Web App Frontend ───────────────────────────────────────────

info "Building Web App frontend..."
if command -v npm &>/dev/null; then
    (cd "$SCRIPT_DIR/web" && npm install --silent && npm run build)
    ok "Web App frontend built."
else
    warn "Skipping Web App build because npm is not installed. Run 'cd web && npm install && npm run build' after installing npm."
fi

# ── Create Data Directory ─────────────────────────────────────────────

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/screentime"
mkdir -p "$DATA_DIR"
ok "Data directory: $DATA_DIR"

# ── Print Next Steps ──────────────────────────────────────────────────

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
echo "     Run:"
echo -e "     ${GREEN}./set-extension-id.sh YOUR_EXTENSION_ID${NC}"
echo "     (or: screentimed install-manifests --extension-id YOUR_EXTENSION_ID)"
echo ""
echo -e "  ${YELLOW}3. Start the daemon:${NC}"
echo -e "     ${GREEN}screentimed &${NC}"
echo ""
echo -e "  ${YELLOW}4. Launch the TUI:${NC}"
echo -e "     ${GREEN}screentime${NC}"
echo ""
echo -e "  ${YELLOW}5. Launch the Web App:${NC}"
echo -e "     ${GREEN}screentime-web${NC}"
echo ""
echo -e "  ${YELLOW}6. Auto-start on login:${NC}"
echo "     Add this to your hyprland.conf (or desktop autostart):"
echo -e "     ${GREEN}exec-once = screentimed${NC}"
echo ""
echo -e "${BLUE}Useful commands:${NC}"
echo "  Status: screentimed status"
echo "  Logs:   tail -f $DATA_DIR/logs/screentimed.log"
echo "  Stop:   pkill -f screentimed"
echo ""
