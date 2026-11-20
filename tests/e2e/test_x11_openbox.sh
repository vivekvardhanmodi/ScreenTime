#!/usr/bin/env bash
# Headless X11 (xvfb + openbox + xdotool) Protocol End-to-End Test Harness
# Spawns an X11 virtual frame buffer with Openbox window manager,
# runs screentimed with x11 backend, switches active window focus
# via xdotool, and asserts SQLite database output.

set -euo pipefail

# If no X11 display is active, wrap execution in xvfb-run
if [[ -z "${DISPLAY:-}" ]]; then
    if ! command -v xvfb-run &>/dev/null; then
        echo "ERROR: xvfb-run is required when DISPLAY is not set." >&2
        exit 1
    fi
    echo "No DISPLAY environment variable detected. Running under xvfb-run..."
    exec xvfb-run -a -s "-screen 0 1024x768x24" "$0" "$@"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TEST_DIR="$(mktemp -d /tmp/screentime-x11-e2e.XXXXXX)"

cleanup() {
    echo "Cleaning up X11 E2E test resources..."
    if [[ -n "${APP1_PID:-}" ]]; then kill -9 "$APP1_PID" 2>/dev/null || true; fi
    if [[ -n "${APP2_PID:-}" ]]; then kill -9 "$APP2_PID" 2>/dev/null || true; fi
    if [[ -n "${DAEMON_PID:-}" ]]; then kill -9 "$DAEMON_PID" 2>/dev/null || true; fi
    if [[ -n "${OPENBOX_PID:-}" ]]; then kill -9 "$OPENBOX_PID" 2>/dev/null || true; fi
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

if ! command -v openbox &>/dev/null; then
    echo "ERROR: openbox is required for headless X11 E2E testing." >&2
    exit 1
fi

if ! command -v xdotool &>/dev/null; then
    echo "ERROR: xdotool is required for X11 focus switching." >&2
    exit 1
fi

echo "Starting Openbox window manager on DISPLAY=$DISPLAY..."
openbox > "$TEST_DIR/openbox.log" 2>&1 &
OPENBOX_PID=$!
sleep 1

TEST_DB="$TEST_DIR/test_x11_screentime.db"
TEST_SOCKET="$TEST_DIR/screentime.sock"

echo "Starting screentimed with x11 backend..."
"$SCREENTIMED_BIN" --platform x11 --db "$TEST_DB" --socket "$TEST_SOCKET" --log-console --log-level debug > "$TEST_DIR/screentimed.log" 2>&1 &
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

# Launch test applications
echo "Launching X11 test applications..."
if command -v xterm &>/dev/null; then
    xterm -class "X11AppOne" -title "X11 Window 1" -e sleep 60 &
    APP1_PID=$!
    sleep 1
    xterm -class "X11AppTwo" -title "X11 Window 2" -e sleep 60 &
    APP2_PID=$!
else
    python3 -c "
import sys, time
try:
    import tkinter as tk
    root = tk.Tk(className='X11AppOne')
    root.title('X11 Window 1')
    root.mainloop()
except Exception:
    time.sleep(60)
" &
    APP1_PID=$!
    sleep 1
    python3 -c "
import sys, time
try:
    import tkinter as tk
    root = tk.Tk(className='X11AppTwo')
    root.title('X11 Window 2')
    root.mainloop()
except Exception:
    time.sleep(60)
" &
    APP2_PID=$!
fi

sleep 2

# Search and switch focus using xdotool
echo "Searching for window IDs with xdotool..."
WIN1=""
WIN2=""
for _ in $(seq 1 10); do
    WIN1="$(xdotool search --class "X11AppOne" 2>/dev/null | head -n 1 || true)"
    WIN2="$(xdotool search --class "X11AppTwo" 2>/dev/null | head -n 1 || true)"
    if [[ -n "$WIN1" && -n "$WIN2" ]]; then
        break
    fi
    sleep 0.5
done

if [[ -n "$WIN1" ]]; then
    echo "Activating window 1 ($WIN1)..."
    xdotool windowactivate --sync "$WIN1" 2>/dev/null || true
    sleep 2
fi

if [[ -n "$WIN2" ]]; then
    echo "Activating window 2 ($WIN2)..."
    xdotool windowactivate --sync "$WIN2" 2>/dev/null || true
    sleep 2
fi

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
    "SELECT id, start_time, end_time, app_class, app_title FROM window_activity ORDER BY id ASC"
).fetchall()

print(f"Total recorded sessions in {db_path}: {len(rows)}")
for r in rows:
    duration = r[2] - r[1]
    print(f"  ID={r[0]}: class='{r[3]}', title='{r[4]}', duration={duration:.2f}s")

if len(rows) < 2:
    print(f"ERROR: Expected at least 2 sessions, found {len(rows)}", file=sys.stderr)
    sys.exit(1)

classes = [str(r[3]).lower() for r in rows]
has_app1 = any("x11appone" in c for c in classes)
has_app2 = any("x11apptwo" in c for c in classes)

if not (has_app1 and has_app2):
    print(f"ERROR: Expected test application classes in {classes}", file=sys.stderr)
    sys.exit(1)

for r in rows:
    if r[1] <= 0 or r[2] < r[1]:
        print(f"ERROR: Invalid session timestamp: start={r[1]}, end={r[2]}", file=sys.stderr)
        sys.exit(1)

print("SUCCESS: Headless X11 Openbox E2E assertions passed!")
EOF

echo "X11 Openbox E2E test finished successfully."
