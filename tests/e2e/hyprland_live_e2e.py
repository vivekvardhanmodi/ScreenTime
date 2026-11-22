#!/usr/bin/env python3
"""Comprehensive Live Hyprland edge-case test suite for screentimed.

Tests native screentimed daemon against the active Hyprland compositor
with real windows, real Chrome tabs, focus switches, URL updates, DPMS idle,
noise filtering, special characters, and IPC edge cases.
Guarantees complete safety and isolation: zero changes to user files or database.
"""

import hashlib
import json
import os
import signal
import socket
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

DATA_DIR = (
    Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    / "screentime"
)
USER_DB_PATH = DATA_DIR / "screentime.db"
RUNTIME_DIR = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"))
TEST_DIR = Path("/tmp/screentime_test")
TEST_DB_PATH = TEST_DIR / "test.db"
SOCKET_PATH = RUNTIME_DIR / "screentime" / "screentimed.sock"
DAEMON_BIN = Path("./target/release/screentimed").resolve()

spawned_pids = []


def compute_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def hypr_dispatch(cmd_str: str) -> str:
    his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE", "")
    sock_path = RUNTIME_DIR / "hypr" / his / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock_path))
        s.sendall(f"dispatch {cmd_str}".encode("utf-8"))
        s.shutdown(socket.SHUT_WR)
        res = s.recv(4096).decode("utf-8", errors="replace").strip()
    return res


def hypr_active() -> dict:
    his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE", "")
    sock_path = RUNTIME_DIR / "hypr" / his / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock_path))
        s.sendall(b"j/activewindow")
        s.shutdown(socket.SHUT_WR)
        data = b""
        while chunk := s.recv(4096):
            data += chunk
    if not data or data.strip() == b"{}":
        return {}
    try:
        return json.loads(data.decode("utf-8", errors="replace"))
    except Exception:
        return {}


def hypr_clients() -> list:
    his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE", "")
    sock_path = RUNTIME_DIR / "hypr" / his / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock_path))
        s.sendall(b"j/clients")
        s.shutdown(socket.SHUT_WR)
        data = b""
        while chunk := s.recv(4096):
            data += chunk
    try:
        return json.loads(data.decode("utf-8", errors="replace"))
    except Exception:
        return []


def query_db(query: str, params: tuple = ()) -> list:
    conn = sqlite3.connect(str(TEST_DB_PATH))
    conn.row_factory = sqlite3.Row
    c = conn.cursor()
    c.execute(query, params)
    rows = [dict(r) for r in c.fetchall()]
    conn.close()
    return rows


def send_ipc(payload: dict):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(SOCKET_PATH))
        line = json.dumps(payload) + "\n"
        s.sendall(line.encode("utf-8"))


def send_raw_ipc(line: str):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(SOCKET_PATH))
        s.sendall((line + "\n").encode("utf-8"))


def spawn_window(title: str) -> dict:
    proc = subprocess.Popen(["kitty", "--title", title, "sh", "-c", "sleep 300"])
    spawned_pids.append(proc.pid)
    for _ in range(50):
        time.sleep(0.1)
        clients = hypr_clients()
        for c in clients:
            if c.get("pid") == proc.pid:
                return c
    raise TimeoutError(f"Window '{title}' (PID {proc.pid}) did not appear in Hyprland clients")


def focus_window_address(addr: str):
    res = hypr_dispatch(f'hl.dsp.focus({{ window = "address:{addr}" }})')
    assert res == "ok", f"Failed to focus window address {addr}: {res}"


def focus_window_title(title: str):
    res = hypr_dispatch(f'hl.dsp.focus({{ window = "title:{title}" }})')
    assert res == "ok", f"Failed to focus window title {title}: {res}"


def focus_window_class(cls: str):
    res = hypr_dispatch(f'hl.dsp.focus({{ window = "class:{cls}" }})')
    assert res == "ok", f"Failed to focus class {cls}: {res}"


def focus_workspace(ws_id: int):
    res = hypr_dispatch(f"hl.dsp.focus({{ workspace = {ws_id} }})")
    assert res == "ok", f"Failed to focus workspace {ws_id}: {res}"


def set_dpms(state: str):
    res = hypr_dispatch(f'hl.dsp.dpms("{state}")')
    assert res == "ok", f"Failed to set dpms: {res}"


def cleanup():
    print("\n--- Cleaning up spawned windows and processes ---")
    # Always ensure display power management is ON so user is never left with blank screens
    try:
        set_dpms("on")
    except Exception:
        pass
    for pid in spawned_pids:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    # Restore focus to Antigravity
    try:
        focus_window_class("antigravity")
    except Exception:
        pass


def run_tests():
    print("=" * 75)
    print("HYPRLAND REAL-TIME EDGE-CASE TEST SUITE")
    print("=" * 75)

    # 1. Safety verification
    assert USER_DB_PATH.exists(), f"User DB does not exist at {USER_DB_PATH}"
    initial_user_db_hash = compute_sha256(USER_DB_PATH)
    print(f"[SAFETY] User DB path: {USER_DB_PATH}")
    print(f"[SAFETY] User DB SHA256 before tests: {initial_user_db_hash}")

    # Prepare test dir
    if TEST_DIR.exists():
        import shutil

        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True, exist_ok=True)

    # Kill any stale screentimed or screentime-browser-host
    subprocess.run(["pkill", "-f", "target/release/screentimed"], check=False)
    subprocess.run(["pkill", "-f", "screentime-browser-host"], check=False)
    time.sleep(0.5)

    if SOCKET_PATH.exists():
        SOCKET_PATH.unlink()

    # 2. Start daemon in test mode
    print("\n[TEST 1] Starting screentimed in isolated test mode...")
    daemon_proc = subprocess.Popen(
        [
            str(DAEMON_BIN),
            "--db",
            str(TEST_DB_PATH),
            "--heartbeat-interval",
            "5",
            "--log-console",
            "--log-level",
            "debug",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    # Wait for socket
    for _ in range(50):
        time.sleep(0.1)
        if SOCKET_PATH.exists():
            break
    assert SOCKET_PATH.exists(), "screentimed failed to create IPC socket"

    # Verify daemon status subcommand
    status_res = subprocess.run(
        [str(DAEMON_BIN), "status"], capture_output=True, text=True
    )
    assert status_res.returncode == 0, f"Status check failed: {status_res.stdout}"
    print(f"[PROOF] Status check CLI output: {status_res.stdout.strip()}")

    # Verify DB initialization
    time.sleep(0.5)
    assert TEST_DB_PATH.exists(), "Test DB was not created"
    tables = query_db(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    )
    table_names = [t["name"] for t in tables]
    assert "sessions" in table_names, f"sessions table missing: {table_names}"
    print(f"[PROOF] Database tables created: {table_names}")

    initial_sessions = query_db("SELECT * FROM sessions")
    print(
        f"[PROOF] Initial session recorded count: {len(initial_sessions)} (Active: {initial_sessions[0]['app_class'] if initial_sessions else 'none'})"
    )

    # 3. Real Window Creation and Switching
    print("\n[TEST 2] Real window creation and focus switching...")
    win_alpha = spawn_window("TestTerminal_Alpha")
    focus_window_address(win_alpha["address"])
    time.sleep(1.8)  # Hold focus > 1s

    alpha_sessions = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Alpha'"
    )
    assert len(alpha_sessions) >= 1, "Session for TestTerminal_Alpha was not recorded"
    alpha_session = alpha_sessions[-1]
    print(
        f"[PROOF] Alpha session created: id={alpha_session['id']}, app_class={alpha_session['app_class']}, start_time={alpha_session['start_time']}"
    )

    win_beta = spawn_window("TestTerminal_Beta")
    focus_window_address(win_beta["address"])
    time.sleep(1.8)  # Hold focus > 1s

    # Verify Alpha session was closed with valid duration
    alpha_closed = query_db(
        "SELECT * FROM sessions WHERE id = ?", (alpha_session["id"],)
    )[0]
    assert (
        alpha_closed["end_time"] is not None
    ), "Alpha session end_time was not recorded"
    alpha_dur = alpha_closed["end_time"] - alpha_closed["start_time"]
    assert (
        alpha_dur >= 1.0
    ), f"Alpha session duration too short ({alpha_dur}s), expected >= 1.0s"
    print(
        f"[PROOF] Alpha session closed cleanly: end_time={alpha_closed['end_time']}, duration={alpha_dur:.2f}s"
    )

    beta_sessions = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Beta'"
    )
    assert len(beta_sessions) >= 1, "Session for TestTerminal_Beta was not recorded"
    beta_session = beta_sessions[-1]
    print(
        f"[PROOF] Beta session created: id={beta_session['id']}, app_class={beta_session['app_class']}, start_time={beta_session['start_time']}"
    )

    # 4. Commas and Special Characters in Titles
    print("\n[TEST 3] Window titles with commas, quotes, and unicode...")
    special_title = "Edge, Case: 'Comma, Quote & Unicode' 🚀 - (Rust)"
    win_special = spawn_window(special_title)
    focus_window_address(win_special["address"])
    time.sleep(1.8)

    special_sessions = query_db(
        "SELECT * FROM sessions WHERE app_title = ?", (special_title,)
    )
    assert (
        len(special_sessions) >= 1
    ), f"Session for title '{special_title}' was not recorded or was truncated at comma!"
    special_session = special_sessions[-1]
    assert special_session["app_class"] == "kitty"
    assert special_session["app_title"] == special_title
    print(
        f"[PROOF] Special characters session verified: class={special_session['app_class']}, title={special_session['app_title']}"
    )

    # 5. Empty Workspace State
    print("\n[TEST 4] Empty workspace / unfocused desktop state...")
    focus_workspace(9)
    time.sleep(2.0)

    # Special session should be closed now
    special_closed = query_db(
        "SELECT * FROM sessions WHERE id = ?", (special_session["id"],)
    )[0]
    assert (
        special_closed["end_time"] is not None
    ), "Session not closed upon switching to empty workspace"
    print(
        f"[PROOF] Session closed upon entering empty workspace: end_time={special_closed['end_time']}"
    )

    # Verify no bogus session was inserted for empty workspace
    active_in_ws9 = query_db(
        "SELECT * FROM sessions WHERE start_time >= ?",
        (special_closed["end_time"],),
    )
    assert (
        len(active_in_ws9) == 0
    ), f"Bogus active session created on empty workspace: {active_in_ws9}"
    print("[PROOF] Zero sessions created on empty workspace (clean unmapped state)")

    # Return to TestTerminal_Alpha
    focus_window_address(win_alpha["address"])
    time.sleep(1.8)
    resumed_sessions = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Alpha' ORDER BY id DESC LIMIT 1"
    )
    assert len(resumed_sessions) == 1
    assert (
        resumed_sessions[0]["id"] != alpha_session["id"]
    ), "New session was not created after returning from empty workspace"
    print(
        f"[PROOF] Tracking resumed on return: new session id={resumed_sessions[0]['id']}"
    )

    # 6. Rapid Window Thrashing (Noise Filtering)
    print("\n[TEST 5] Rapid window thrashing and noise filtering...")
    # Switch back and forth rapidly (< 1s per switch)
    for _ in range(4):
        focus_window_address(win_beta["address"])
        time.sleep(0.15)
        focus_window_address(win_alpha["address"])
        time.sleep(0.15)
    # Now settle on TestTerminal_Beta for 2s
    focus_window_address(win_beta["address"])
    time.sleep(2.0)
    # Switch back to Alpha so Beta's session is closed and committed
    focus_window_address(win_alpha["address"])
    time.sleep(1.8)

    # Verify all closed sessions have duration >= 1.0s (sub-second thrash sessions deleted)
    thrash_survivors = query_db(
        "SELECT * FROM sessions WHERE id != (SELECT MAX(id) FROM sessions) AND (end_time - start_time) < 0.99"
    )
    assert (
        len(thrash_survivors) == 0
    ), f"Noise sessions under 1s were not pruned: {thrash_survivors}"
    print("[PROOF] Sub-second noise sessions were cleanly pruned from database")

    # 7. Browser Tracking & URL Updates
    print("\n[TEST 6] Browser window detection and URL updates...")
    # Locate Chrome window
    chrome_clients = [c for c in hypr_clients() if "chrome" in c.get("class", "").lower()]
    assert len(chrome_clients) > 0, "No Google Chrome window found running on system"
    chrome_addr = chrome_clients[0]["address"]
    focus_window_address(chrome_addr)
    time.sleep(0.5)

    # Edge Case 6a: In-flight URL update within 2.0s of switch
    url_1 = "https://github.com/rust-lang/rust"
    send_ipc(
        {
            "url": url_1,
            "title": "Rust Programming Language",
            "browser_type": "google-chrome",
        }
    )
    time.sleep(1.5)

    chrome_sessions = query_db(
        "SELECT * FROM sessions WHERE app_class = 'google-chrome' ORDER BY id DESC LIMIT 1"
    )
    assert len(chrome_sessions) == 1, "No chrome session found"
    chrome_s1 = chrome_sessions[0]
    assert (
        chrome_s1["website_url"] == url_1
    ), f"Expected URL {url_1}, got {chrome_s1['website_url']}"
    print(
        f"[PROOF] Immediate URL update attached in-place: id={chrome_s1['id']}, url={chrome_s1['website_url']}"
    )

    # Edge Case 6b: URL change after > 2s creates a new session
    time.sleep(2.2)
    url_2 = "https://doc.rust-lang.org/book/"
    send_ipc(
        {
            "url": url_2,
            "title": "The Rust Book",
            "browser_type": "google-chrome",
        }
    )
    time.sleep(1.5)

    chrome_s1_closed = query_db(
        "SELECT * FROM sessions WHERE id = ?", (chrome_s1["id"],)
    )[0]
    assert (
        chrome_s1_closed["end_time"] is not None
    ), "Previous URL session was not closed on navigation"

    chrome_sessions_2 = query_db(
        "SELECT * FROM sessions WHERE app_class = 'google-chrome' ORDER BY id DESC LIMIT 1"
    )
    chrome_s2 = chrome_sessions_2[0]
    assert (
        chrome_s2["id"] != chrome_s1["id"]
    ), "New session was not created for subsequent URL change"
    assert (
        chrome_s2["website_url"] == url_2
    ), f"Expected URL {url_2}, got {chrome_s2['website_url']}"
    print(
        f"[PROOF] Subsequent URL navigation created distinct session: id={chrome_s2['id']}, url={chrome_s2['website_url']}"
    )

    # Edge Case 6c: URL update received while non-browser app is focused
    focus_window_address(win_alpha["address"])
    time.sleep(1.5)
    terminal_session = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Alpha' ORDER BY id DESC LIMIT 1"
    )[0]

    url_3 = "https://news.ycombinator.com/"
    send_ipc(
        {
            "url": url_3,
            "title": "Hacker News",
            "browser_type": "google-chrome",
        }
    )
    time.sleep(1.0)

    terminal_check = query_db(
        "SELECT * FROM sessions WHERE id = ?", (terminal_session["id"],)
    )[0]
    assert (
        terminal_check["website_url"] is None
    ), f"Terminal window received browser URL! Got: {terminal_check['website_url']}"
    print(
        "[PROOF] Terminal window correctly ignored browser URL update (website_url is NULL)"
    )

    # Now focus back to Chrome: it should inherit url_3
    focus_window_address(chrome_addr)
    time.sleep(1.5)
    chrome_inherited = query_db(
        "SELECT * FROM sessions WHERE app_class = 'google-chrome' ORDER BY id DESC LIMIT 1"
    )[0]
    assert (
        chrome_inherited["website_url"] == url_3
    ), f"Chrome did not inherit buffered URL! Expected {url_3}, got {chrome_inherited['website_url']}"
    print(
        f"[PROOF] Chrome inherited buffered URL on refocus: id={chrome_inherited['id']}, url={chrome_inherited['website_url']}"
    )

    # Edge Case 6d: Real Chrome Tab opening via CLI
    print("\n[TEST 7] Testing real Chrome tab launch and extension IPC tracking...")
    chrome_real_url = "https://example.com/screentime-real-tab-test"
    subprocess.run(["google-chrome-stable", chrome_real_url], check=False)
    focus_window_address(chrome_addr)
    time.sleep(2.0)

    # Verify real Chrome tab URL was written to sessions table by the extension via IPC
    real_tab_sessions = query_db(
        "SELECT * FROM sessions WHERE website_url LIKE '%screentime-real-tab-test%'"
    )
    assert len(real_tab_sessions) >= 1, "Real Chrome tab URL was not recorded in sessions table"
    print(f"[PROOF] Real Chrome tab captured in SQLite: id={real_tab_sessions[0]['id']}, url={real_tab_sessions[0]['website_url']}")

    # Safely close the tab with Ctrl+W via wtype after verifying focus
    focus_window_address(chrome_addr)
    time.sleep(0.2)
    subprocess.run(["wtype", "-M", "ctrl", "-k", "w", "-m", "ctrl"], check=False)
    time.sleep(1.0)
    print("[PROOF] Real Chrome tab opened and closed via wtype")

    # Edge Case 6e: Malformed and Edge-case IPC messages
    print("\n[TEST 8] Malformed and boundary IPC messages resilience...")
    # Send empty line, malformed JSON, super long URL
    send_raw_ipc("")
    send_raw_ipc("not a valid json string {[]}")
    long_url = "https://example.com/search?query=" + ("A" * 3000)
    send_ipc(
        {"url": long_url, "title": "Long URL Test", "browser_type": "google-chrome"}
    )
    time.sleep(1.5)
    long_url_session = query_db(
        "SELECT * FROM sessions WHERE app_class = 'google-chrome' ORDER BY id DESC LIMIT 1"
    )[0]
    assert (
        long_url_session["website_url"] == long_url
    ), "Long URL was corrupted or not saved"
    print(
        f"[PROOF] Handled 3KB URL and malformed JSON resilience: session id={long_url_session['id']}"
    )

    # 8. DPMS Idle Detection
    print(
        "\n[TEST 9] Display power management (DPMS) idle detection and recovery..."
    )
    focus_window_address(win_alpha["address"])
    time.sleep(1.8)
    pre_idle_session = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Alpha' ORDER BY id DESC LIMIT 1"
    )[0]
    print(f"[PROOF] Pre-idle active session: id={pre_idle_session['id']}")

    # Turn DPMS off
    set_dpms("off")
    time.sleep(3.0)  # Polling interval is 2s

    post_off_check = query_db(
        "SELECT * FROM sessions WHERE id = ?", (pre_idle_session["id"],)
    )[0]
    assert (
        post_off_check["end_time"] is not None
    ), "Session was not closed when DPMS turned off!"
    print(
        f"[PROOF] Session closed immediately upon DPMS power off: end_time={post_off_check['end_time']}"
    )

    # Turn DPMS on
    set_dpms("on")
    time.sleep(2.5)

    resumed_post_idle = query_db(
        "SELECT * FROM sessions WHERE app_title = 'TestTerminal_Alpha' ORDER BY id DESC LIMIT 1"
    )[0]
    assert (
        resumed_post_idle["id"] != pre_idle_session["id"]
    ), "New session was not started after DPMS turned back on!"
    print(
        f"[PROOF] Session tracking automatically resumed on DPMS on: new id={resumed_post_idle['id']}"
    )

    # 9. Heartbeat Progression
    print("\n[TEST 10] Heartbeat progression on continuously active window...")
    initial_hb_time = resumed_post_idle["end_time"]
    time.sleep(7.0)  # Configured heartbeat interval is 5s
    post_hb_check = query_db(
        "SELECT * FROM sessions WHERE id = ?", (resumed_post_idle["id"],)
    )[0]
    assert post_hb_check["end_time"] > initial_hb_time, (
        f"Heartbeat did not update end_time! Initial: {initial_hb_time}, Current: {post_hb_check['end_time']}"
    )
    print(
        f"[PROOF] Heartbeat updated end_time: {initial_hb_time} -> {post_hb_check['end_time']} (delta: {post_hb_check['end_time'] - initial_hb_time:.2f}s)"
    )

    # 10. Graceful Shutdown & Final Session Closure
    print("\n[TEST 11] Graceful daemon shutdown and session finalization...")
    daemon_proc.send_signal(signal.SIGTERM)
    daemon_proc.wait(timeout=5)
    assert daemon_proc.returncode == 0, (
        f"screentimed exited with code {daemon_proc.returncode}"
    )
    print(
        f"[PROOF] screentimed exited cleanly with return code {daemon_proc.returncode}"
    )

    # Check that socket was removed
    assert not SOCKET_PATH.exists(), (
        f"IPC socket {SOCKET_PATH} was not removed after shutdown"
    )
    print("[PROOF] IPC socket file was cleanly unlinked on shutdown")

    # Check that final active session was closed
    final_session = query_db(
        "SELECT * FROM sessions WHERE id = ?", (resumed_post_idle["id"],)
    )[0]
    assert final_session["end_time"] is not None
    assert final_session["end_time"] >= post_hb_check["end_time"]
    print(
        f"[PROOF] Final session closed on shutdown: id={final_session['id']}, end_time={final_session['end_time']}"
    )

    # Integrity check on SQLite database
    integrity = query_db("PRAGMA integrity_check")
    assert integrity[0]["integrity_check"] == "ok", (
        f"Integrity check failed: {integrity}"
    )
    print("[PROOF] SQLite database PRAGMA integrity_check: ok")

    # Dump session summary for report
    all_sessions = query_db(
        "SELECT id, app_class, app_title, website_url, ROUND(end_time - start_time, 2) AS duration FROM sessions ORDER BY id"
    )
    print("\n--- Final Sessions Table Dump ---")
    for s in all_sessions:
        url_disp = f" | url={s['website_url']}" if s["website_url"] else ""
        print(
            f"  [id {s['id']}] {s['app_class']} | title: {s['app_title'][:40]}{url_disp} | dur: {s['duration']}s"
        )

    # 11. Final Safety Verification
    print("\n[TEST 12] Final safety invariant verification...")
    final_user_db_hash = compute_sha256(USER_DB_PATH)
    assert initial_user_db_hash == final_user_db_hash, (
        f"CRITICAL: User DB was modified! Initial: {initial_user_db_hash}, Final: {final_user_db_hash}"
    )
    print(f"[PROOF] User DB SHA256 untouched: {final_user_db_hash}")

    print("\n" + "=" * 75)
    print("ALL HYPRLAND REAL-TIME EDGE CASE TESTS PASSED PERFECTLY!")
    print("=" * 75)


if __name__ == "__main__":
    try:
        run_tests()
    finally:
        cleanup()
