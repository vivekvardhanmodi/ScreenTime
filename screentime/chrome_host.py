#!/usr/bin/env python3
"""Chrome Native Messaging Host for ScreenTime.

Receives active tab URL from the Chrome extension and writes
it to a state file that the daemon reads.

Protocol: Chrome sends/receives length-prefixed JSON on stdin/stdout.
Each message is preceded by a 4-byte native-order unsigned integer length.
"""

import json
import os
import struct
import sys
from pathlib import Path


def get_runtime_dir() -> Path:
    """Get the ScreenTime runtime directory."""
    xdg = os.environ.get("XDG_RUNTIME_DIR")
    if xdg:
        d = Path(xdg) / "screentime"
    else:
        d = Path(f"/tmp/screentime-{os.getuid()}")
    d.mkdir(parents=True, exist_ok=True)
    return d


CHROME_URL_FILE = get_runtime_dir() / "chrome_url"
LOG_FILE = get_runtime_dir() / "chrome_host.log"


def log(msg: str):
    """Simple file logging."""
    try:
        with open(LOG_FILE, "a") as f:
            f.write(f"{msg}\n")
    except Exception:
        pass


def read_message() -> dict | None:
    """Read a single native messaging message from stdin."""
    raw_length = sys.stdin.buffer.read(4)
    if not raw_length or len(raw_length) < 4:
        return None
    message_length = struct.unpack("=I", raw_length)[0]
    if message_length == 0:
        return None
    raw_message = sys.stdin.buffer.read(message_length)
    if not raw_message:
        return None
    return json.loads(raw_message.decode("utf-8"))


def send_message(message: dict):
    """Send a native messaging message to Chrome."""
    encoded = json.dumps(message).encode("utf-8")
    sys.stdout.buffer.write(struct.pack("=I", len(encoded)))
    sys.stdout.buffer.write(encoded)
    sys.stdout.buffer.flush()


def write_url_state(domain: str | None, title: str | None):
    """Write the current Chrome URL state to the shared file."""
    state = {
        "domain": domain,
        "title": title,
        "pid": os.getpid(),
    }
    # Atomic write: write to temp file then rename
    tmp_file = CHROME_URL_FILE.with_suffix(".tmp")
    with open(tmp_file, "w") as f:
        json.dump(state, f)
    tmp_file.rename(CHROME_URL_FILE)


def clear_url_state():
    """Clear the URL state file when Chrome disconnects."""
    try:
        CHROME_URL_FILE.unlink(missing_ok=True)
    except Exception:
        pass


def main():
    """Main loop: read messages from Chrome, update state file."""
    log("Native messaging host started")

    try:
        while True:
            message = read_message()
            if message is None:
                log("Chrome disconnected (stdin closed)")
                break

            msg_type = message.get("type", "")

            if msg_type == "url_update":
                domain = message.get("domain")
                title = message.get("title")
                write_url_state(domain, title)
                send_message({"status": "ok"})

            elif msg_type == "ping":
                send_message({"status": "pong"})

            else:
                log(f"Unknown message type: {msg_type}")
                send_message({"status": "error", "message": f"Unknown type: {msg_type}"})

    except Exception as e:
        log(f"Error: {e}")
    finally:
        clear_url_state()
        log("Native messaging host exiting")


if __name__ == "__main__":
    main()
