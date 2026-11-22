#!/usr/bin/env bash
# Headless KDE Plasma (KWin Wayland) Protocol End-to-End Test Harness
# Spawns headless kwin_wayland --virtual with private D-Bus session,
# runs screentimed with --platform kde, launches Wayland applications,
# verifies KWin ECMAScript D-Bus event handling, and asserts SQLite rows.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TEST_DIR="$(mktemp -d /tmp/screentime-kde-e2e.XXXXXX)"
export XDG_RUNTIME_DIR="$TEST_DIR/runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 0700 "$XDG_RUNTIME_DIR"

cleanup() {
    local exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        echo "=== KWin Log ==="
        cat "$TEST_DIR/kwin.log" 2>/dev/null || true
        echo "=== Screentimed Log ==="
        cat "$TEST_DIR/screentimed.log" 2>/dev/null || true
    fi
    echo "Cleaning up KDE E2E test resources..."
    if [[ -n "${APP1_PID:-}" ]]; then kill -9 "$APP1_PID" 2>/dev/null || true; fi
    if [[ -n "${APP2_PID:-}" ]]; then kill -9 "$APP2_PID" 2>/dev/null || true; fi
    if [[ -n "${DAEMON_PID:-}" ]]; then kill -9 "$DAEMON_PID" 2>/dev/null || true; fi
    if [[ -n "${KWIN_PID:-}" ]]; then kill -9 "$KWIN_PID" 2>/dev/null || true; fi
    if [[ -n "${DBUS_PID:-}" ]]; then kill -9 "$DBUS_PID" 2>/dev/null || true; fi
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT INT TERM

# Locate screentimed binary
SCREENTIMED_BIN=""
if [[ -x "$WORKSPACE_ROOT/target/release/screentimed" ]]; then
    SCREENTIMED_BIN="$WORKSPACE_ROOT/target/release/screentimed"
elif [[ -x "$WORKSPACE_ROOT/target/debug/screentimed" ]]; then
    SCREENTIMED_BIN="$WORKSPACE_ROOT/target/debug/screentimed"
elif command -v screentimed &>/dev/null; then
    SCREENTIMED_BIN="$(command -v screentimed)"
else
    echo "screentimed binary not found. Building workspace..."
    (cd "$WORKSPACE_ROOT" && cargo build --bin screentimed)
    SCREENTIMED_BIN="$WORKSPACE_ROOT/target/debug/screentimed"
fi

if ! command -v kwin_wayland &>/dev/null; then
    echo "ERROR: kwin_wayland is required for headless KDE E2E testing." >&2
    exit 1
fi

# Start isolated private D-Bus session bus
DBUS_SOCK="$TEST_DIR/dbus.sock"
dbus-daemon --session --address="unix:path=$DBUS_SOCK" --nofork --nopidfile > "$TEST_DIR/dbus.log" 2>&1 &
DBUS_PID=$!
export DBUS_SESSION_BUS_ADDRESS="unix:path=$DBUS_SOCK"

# Wait for D-Bus socket
for _ in $(seq 1 30); do
    if [[ -S "$DBUS_SOCK" ]]; then
        break
    fi
    sleep 0.1
done

echo "Starting headless KWin Wayland compositor..."
kwin_wayland --virtual --no-lockscreen --no-kactivities > "$TEST_DIR/kwin.log" 2>&1 &
KWIN_PID=$!

echo "Waiting for KWin to register on D-Bus..."
KWIN_READY=0
for _ in $(seq 1 50); do
    if dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames 2>/dev/null | grep -q "org.kde.KWin"; then
        KWIN_READY=1
        break
    fi
    sleep 0.2
done

if [[ $KWIN_READY -ne 1 ]]; then
    echo "ERROR: KWin failed to register org.kde.KWin on D-Bus within 10 seconds" >&2
    cat "$TEST_DIR/kwin.log" || true
    exit 1
fi
echo "KWin initialized and registered on D-Bus."

# Detect Wayland display socket
WAYLAND_DISPLAY=""
for sock in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do
    if [[ -S "$sock" ]]; then
        WAYLAND_DISPLAY="$(basename "$sock")"
        break
    fi
done

if [[ -z "$WAYLAND_DISPLAY" ]]; then
    echo "ERROR: Failed to detect Wayland display socket in $XDG_RUNTIME_DIR" >&2
    ls -la "$XDG_RUNTIME_DIR" || true
    exit 1
fi
export WAYLAND_DISPLAY
echo "Detected active WAYLAND_DISPLAY=$WAYLAND_DISPLAY"

TEST_DB="$TEST_DIR/test_kde_screentime.db"
TEST_SOCKET="$TEST_DIR/screentime.sock"

echo "Starting screentimed with KDE backend..."
"$SCREENTIMED_BIN" --platform kde --db "$TEST_DB" --socket "$TEST_SOCKET" --log-console --log-level debug > "$TEST_DIR/screentimed.log" 2>&1 &
DAEMON_PID=$!

echo "Waiting for screentimed to initialize IPC socket..."
DAEMON_READY=0
for _ in $(seq 1 30); do
    if "$SCREENTIMED_BIN" status --socket "$TEST_SOCKET" &>/dev/null; then
        DAEMON_READY=1
        break
    fi
    sleep 0.5
done

if [[ $DAEMON_READY -ne 1 ]]; then
    echo "ERROR: screentimed failed to start within 15 seconds" >&2
    cat "$TEST_DIR/screentimed.log" || true
    exit 1
fi
echo "screentimed running and listening."

# Launch test applications on KWin
echo "Launching test applications..."
if command -v foot &>/dev/null; then
    foot -a "KdeAppOne" -T "KDE Window 1" sleep 60 &
    APP1_PID=$!
    sleep 2
    foot -a "KdeAppTwo" -T "KDE Window 2" sleep 60 &
    APP2_PID=$!
elif command -v xterm &>/dev/null; then
    xterm -class "KdeAppOne" -title "KDE Window 1" -e sleep 60 &
    APP1_PID=$!
    sleep 2
    xterm -class "KdeAppTwo" -title "KDE Window 2" -e sleep 60 &
    APP2_PID=$!
fi

sleep 3

# Shut down screentimed cleanly
echo "Stopping screentimed..."
kill -TERM "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true

# Assert SQLite database contents
echo "Asserting SQLite database rows..."
python3 - "$TEST_DB" << 'EOF'
import sqlite3, sys

db_path = sys.argv[1]
conn = sqlite3.connect(db_path)
cur = conn.cursor()

rows = cur.execute(
    "SELECT id, start_time, end_time, app_class, app_title FROM sessions ORDER BY id ASC"
).fetchall()

print(f"Total recorded sessions in {db_path}: {len(rows)}")
for r in rows:
    duration = r[2] - r[1]
    print(f"  ID={r[0]}: class='{r[3]}', title='{r[4]}', duration={duration:.2f}s")

if len(rows) < 1:
    print(f"ERROR: Expected at least 1 session, found {len(rows)}", file=sys.stderr)
    sys.exit(1)

classes = [str(r[3]).lower() for r in rows]
has_app = any("kdeapp" in c for c in classes)

if not has_app:
    print(f"ERROR: Expected test application class in {classes}", file=sys.stderr)
    sys.exit(1)

for r in rows:
    if r[1] <= 0 or r[2] < r[1]:
        print(f"ERROR: Invalid session timestamp: start={r[1]}, end={r[2]}", file=sys.stderr)
        sys.exit(1)

print("SUCCESS: Headless KDE KWin E2E assertions passed!")
EOF

echo "KDE KWin E2E test finished successfully."
