"""Hyprland IPC window tracker.

Connects to Hyprland's event socket (socket2) for real-time
active window change notifications. Also provides a one-shot
query for the current active window via hyprctl.
"""

import asyncio
import json
import logging
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Callable, Awaitable

log = logging.getLogger(__name__)


@dataclass
class WindowInfo:
    """Information about the currently active window."""
    app_class: str
    title: str

    def __eq__(self, other):
        if not isinstance(other, WindowInfo):
            return NotImplemented
        return self.app_class == other.app_class and self.title == other.title


def _get_socket_path() -> Path:
    """Find the Hyprland IPC socket2 path."""
    his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    if not his:
        raise RuntimeError(
            "HYPRLAND_INSTANCE_SIGNATURE not set — is Hyprland running?"
        )

    # Try XDG_RUNTIME_DIR first (newer Hyprland), then /tmp
    xdg = os.environ.get("XDG_RUNTIME_DIR")
    if xdg:
        path = Path(xdg) / "hypr" / his / ".socket2.sock"
        if path.exists():
            return path

    path = Path("/tmp") / "hypr" / his / ".socket2.sock"
    if path.exists():
        return path

    raise RuntimeError(
        f"Hyprland socket2 not found for instance {his}. "
        "Checked $XDG_RUNTIME_DIR/hypr/ and /tmp/hypr/"
    )


async def get_active_window() -> Optional[WindowInfo]:
    """Query the current active window via hyprctl (one-shot).

    Returns None if no window is focused or hyprctl fails.
    """
    try:
        proc = await asyncio.create_subprocess_exec(
            "hyprctl", "activewindow", "-j",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )
        try:
            stdout_data, stderr_data = await asyncio.wait_for(proc.communicate(), timeout=5.0)
        except asyncio.TimeoutError:
            proc.kill()
            await proc.wait()
            log.warning("hyprctl activewindow timed out")
            return None

        if proc.returncode != 0:
            log.warning("hyprctl failed: %s", stderr_data.decode().strip())
            return None

        data = json.loads(stdout_data.decode())

        # hyprctl returns an empty object or "class":"" when no window is focused
        app_class = data.get("class", "").strip()
        if not app_class:
            return None

        title = data.get("title", "").strip()
        return WindowInfo(app_class=app_class, title=title)

    except (json.JSONDecodeError, FileNotFoundError, OSError) as e:
        log.warning("Failed to get active window: %s", e)
        return None


class HyprlandTracker:
    """Async listener for Hyprland active window changes via IPC socket2.

    Calls the provided callback whenever the active window changes.
    The callback receives a WindowInfo (or None if no window is focused).
    """

    def __init__(self, on_window_change: Callable[[Optional[WindowInfo]], Awaitable[None]]):
        self._on_window_change = on_window_change
        self._task: Optional[asyncio.Task] = None
        self._running = False

    async def start(self):
        """Start listening for window change events."""
        self._running = True
        self._task = asyncio.create_task(self._listen())
        log.info("Hyprland tracker started")

    async def stop(self):
        """Stop listening."""
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        log.info("Hyprland tracker stopped")

    async def _listen(self):
        """Connect to socket2 and listen for activewindow events."""
        socket_path = _get_socket_path()
        log.info("Connecting to Hyprland socket: %s", socket_path)

        while self._running:
            try:
                reader, writer = await asyncio.open_unix_connection(str(socket_path))
                log.info("Connected to Hyprland IPC")

                buffer = ""
                while self._running:
                    data = await reader.read(4096)
                    if not data:
                        log.warning("Hyprland socket closed, reconnecting...")
                        break

                    buffer += data.decode("utf-8", errors="replace")

                    # Process complete lines
                    while "\n" in buffer:
                        line, buffer = buffer.split("\n", 1)
                        line = line.strip()
                        if not line:
                            continue

                        await self._handle_event(line)

                writer.close()

            except (ConnectionRefusedError, FileNotFoundError, OSError) as e:
                log.warning("Socket connection error: %s, retrying in 2s...", e)

            if self._running:
                await asyncio.sleep(2)

    async def _handle_event(self, line: str):
        """Parse and handle a single IPC event line."""
        # Format: activewindow>>CLASS,TITLE
        if not line.startswith("activewindow>>"):
            return

        payload = line[len("activewindow>>"):]

        if not payload or payload == ",":
            # No active window (e.g., desktop focused)
            await self._on_window_change(None)
            return

        # Split on first comma only — title may contain commas
        parts = payload.split(",", 1)
        app_class = parts[0].strip()
        title = parts[1].strip() if len(parts) > 1 else ""

        if not app_class:
            await self._on_window_change(None)
            return

        window = WindowInfo(app_class=app_class, title=title)
        await self._on_window_change(window)
