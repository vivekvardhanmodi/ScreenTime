#!/usr/bin/env bash
# Headless Wayland (Sway) Protocol End-to-End Test Harness
# Spawns headless Sway with virtual outputs, runs screentimed,
# switches focus between applications, triggers DPMS off/on,
# and asserts SQLite database output.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TEST_DIR="$(mktemp -d /tmp/screentime-wayland-e2e.XXXXXX)"
export XDG_RUNTIME_DIR="$TEST_DIR/runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 0700 "$XDG_RUNTIME_DIR"

export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export SWAYSOCK="$XDG_RUNTIME_DIR/sway-ipc.sock"
export WAYLAND_DISPLAY="wayland-screentime-test"

cleanup() {
    echo "Cleaning up Wayland E2E test resources..."
    if [[ -n "${APP1_PID:-}" ]]; then kill -9 "$APP1_PID" 2>/dev/null || true; fi
    if [[ -n "${APP2_PID:-}" ]]; then kill -9 "$APP2_PID" 2>/dev/null || true; fi
    if [[ -n "${DAEMON_PID:-}" ]]; then kill -9 "$DAEMON_PID" 2>/dev/null || true; fi
    if [[ -n "${SWAY_PID:-}" ]]; then kill -9 "$SWAY_PID" 2>/dev/null || true; fi
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

if ! command -v sway &>/dev/null; then
    echo "ERROR: sway is required for headless Wayland E2E testing." >&2
    exit 1
fi

# Generate minimal headless sway configuration
cat > "$TEST_DIR/sway.conf" << 'EOF'
output HEADLESS-1 resolution 1920x1080 position 0,0
seat seat0 xcursor_theme default
xwayland enable
bar {
    mode invisible
}
EOF

echo "Starting headless Sway compositor..."
sway -c "$TEST_DIR/sway.conf" --unsupported-gpu > "$TEST_DIR/sway.log" 2>&1 &
SWAY_PID=$!

echo "Waiting for Sway compositor to become ready..."
SWAY_READY=0
for _ in $(seq 1 30); do
    if swaymsg -s "$SWAYSOCK" -t get_version &>/dev/null; then
        SWAY_READY=1
        break
    fi
    sleep 0.5
done

if [[ $SWAY_READY -ne 1 ]]; then
    echo "ERROR: Sway failed to initialize within 15 seconds" >&2
    cat "$TEST_DIR/sway.log" || true
    exit 1
fi
echo "Sway compositor initialized successfully."

TEST_DB="$TEST_DIR/test_wayland_screentime.db"
TEST_SOCKET="$TEST_DIR/screentime.sock"

echo "Starting screentimed with wlroots backend..."
"$SCREENTIMED_BIN" --platform wlroots --db "$TEST_DB" --socket "$TEST_SOCKET" --log-console --log-level debug > "$TEST_DIR/screentimed.log" 2>&1 &
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
echo "Launching test applications..."
if command -v foot &>/dev/null; then
    foot -a "WaylandAppOne" -T "Wayland Window 1" sleep 60 &
    APP1_PID=$!
    sleep 1
    foot -a "WaylandAppTwo" -T "Wayland Window 2" sleep 60 &
    APP2_PID=$!
elif command -v xterm &>/dev/null; then
    xterm -class "WaylandAppOne" -title "Wayland Window 1" -e sleep 60 &
    APP1_PID=$!
    sleep 1
    xterm -class "WaylandAppTwo" -title "Wayland Window 2" -e sleep 60 &
    APP2_PID=$!
else
    python3 -c "
import sys, time
try:
    import tkinter as tk
    root = tk.Tk(className='WaylandAppOne')
    root.title('Wayland Window 1')
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
    root = tk.Tk(className='WaylandAppTwo')
    root.title('Wayland Window 2')
    root.mainloop()
except Exception:
    time.sleep(60)
" &
    APP2_PID=$!
fi

sleep 2

# Switch focus between windows using swaymsg
echo "Switching focus to WaylandAppOne..."
swaymsg '[app_id="(?i)WaylandAppOne"] focus' 2>/dev/null || swaymsg '[class="(?i)WaylandAppOne"] focus' 2>/dev/null || true
sleep 2

echo "Switching focus to WaylandAppTwo..."
swaymsg '[app_id="(?i)WaylandAppTwo"] focus' 2>/dev/null || swaymsg '[class="(?i)WaylandAppTwo"] focus' 2>/dev/null || true
sleep 2

# Trigger DPMS power transitions
echo "Triggering DPMS off..."
swaymsg 'output * dpms off'
sleep 2

echo "Triggering DPMS on..."
swaymsg 'output * dpms on'
sleep 2

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
has_app1 = any("waylandappone" in c for c in classes)
has_app2 = any("waylandapptwo" in c for c in classes)

if not (has_app1 and has_app2):
    print(f"ERROR: Expected test application classes in {classes}", file=sys.stderr)
    sys.exit(1)

for r in rows:
    if r[1] <= 0 or r[2] < r[1]:
        print(f"ERROR: Invalid session timestamp: start={r[1]}, end={r[2]}", file=sys.stderr)
        sys.exit(1)

print("SUCCESS: Headless Wayland Sway E2E assertions passed!")
EOF

echo "Wayland Sway E2E test finished successfully."
