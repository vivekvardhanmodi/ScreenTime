"""ScreenTime daemon — main orchestrator.

Coordinates window tracking, idle detection, and browser URL tracking
to log activity sessions into the SQLite database.

Run as: python -m screentime.daemon
Or directly: screentimed
"""

import asyncio
import json
import logging
import os
import signal
import sys
import time
from typing import Optional

from screentime import (
    CHROME_WINDOW_CLASS,
    FIREFOX_WINDOW_CLASS,
    HEARTBEAT_INTERVAL,
    RUNTIME_DIR,
)
from screentime.database import Database
from screentime.idle import IdleDetector
from screentime.tracker import HyprlandTracker, WindowInfo, get_active_window

from pathlib import Path

# ── Logging setup ─────────────────────────────────────────────────────

LOG_FORMAT = "%(asctime)s [%(levelname)s] %(name)s: %(message)s"
LOG_DATE_FORMAT = "%Y-%m-%d %H:%M:%S"


def setup_logging():
    """Configure logging to stderr and file."""
    from screentime import DATA_DIR

    log_file = DATA_DIR / "daemon.log"

    handlers = [
        logging.StreamHandler(sys.stderr),
        logging.FileHandler(str(log_file)),
    ]

    logging.basicConfig(
        level=logging.INFO,
        format=LOG_FORMAT,
        datefmt=LOG_DATE_FORMAT,
        handlers=handlers,
    )


log = logging.getLogger(__name__)


# ── Browser URL reader ────────────────────────────────────────────────

def _browser_url_file(window_class: str) -> Path:
    """Get the URL state file path for the given browser window class."""
    wc = window_class.lower()
    if FIREFOX_WINDOW_CLASS in wc:
        return RUNTIME_DIR / "firefox_url"
    return RUNTIME_DIR / "chrome_url"


def read_browser_url(window_class: str) -> tuple[Optional[str], Optional[str]]:
    """Read the current browser tab URL from the state file.

    Returns (url, title) or (None, None) if unavailable.
    """
    try:
        url_file = _browser_url_file(window_class)
        if not url_file.exists():
            return None, None

        with open(url_file) as f:
            data = json.load(f)

        return data.get("url"), data.get("title")
    except (json.JSONDecodeError, IOError, KeyError):
        return None, None


# ── Session Manager ───────────────────────────────────────────────────

class SessionManager:
    """Manages the lifecycle of activity sessions.

    Coordinates between window changes, idle events, and Chrome URL updates
    to maintain accurate session records in the database.
    """

    def __init__(self, db: Database):
        self.db = db
        self._current_session_id: Optional[int] = None
        self._current_session_app_class: Optional[str] = None
        self._current_window: Optional[WindowInfo] = None
        self._current_url: Optional[str] = None
        self._current_url_title: Optional[str] = None
        self._is_idle = False
        self._is_lid_closed = False
        self._lock = asyncio.Lock()

    def _resolve_app_class(self, window: WindowInfo) -> str:
        """Resolve the effective app class using user-defined title rules."""
        raw_class = window.app_class
        rules = self.db.get_title_rules().get(raw_class)
        if rules and window.title:
            title_lower = window.title.lower()
            for target in rules:
                if title_lower.startswith(target.lower()):
                    return target
        return raw_class

    async def on_window_change(self, window: Optional[WindowInfo]):
        """Handle active window change from Hyprland IPC."""
        async with self._lock:
            if self._is_idle or self._is_lid_closed:
                # If idle or lid closed, just remember the new window for when we resume
                self._current_window = window
                return

            if window is None:
                now = time.time()
                self._close_current_session(now)
                self._current_window = None
                self._current_session_app_class = None
                return

            effective_class = self._resolve_app_class(window)

            # Deduplicate: Skip if both the raw window and the resolved class haven't changed.
            if (
                self._current_window is not None
                and window.app_class == self._current_window.app_class
                and effective_class == self._current_session_app_class
            ):
                return

            now = time.time()

            # Close existing session if any
            self._close_current_session(now)

            self._current_window = window
            self._current_session_app_class = effective_class

            # For browser windows, don't read URL immediately — the extension
            # hasn't had time to send the updated URL yet, so the state file
            # is stale. Start with url=None and schedule a delayed read.
            self._current_url = None
            self._current_url_title = None

            # Start new session
            self._start_session(now, window, effective_class, None, None)

        # Schedule a delayed URL check for browser windows (outside the lock)
        if window and self._is_browser(window):
            asyncio.create_task(self._delayed_url_check(window))

    async def on_idle(self):
        """Handle user going idle."""
        async with self._lock:
            self._is_idle = True
            now = time.time()
            self._close_current_session(now)
            log.info("Idle: closed current session")

    async def on_active(self):
        """Handle user returning from idle."""
        async with self._lock:
            self._is_idle = False
            if self._is_lid_closed:
                return  # Don't resume if lid is still closed
            
            now = time.time()

            # Re-query the current window since it may have changed during idle
            window = get_active_window()
            self._current_window = window

            if window is None:
                return

            effective_class = self._resolve_app_class(window)
            self._current_session_app_class = effective_class
            self._current_url = None
            self._current_url_title = None
            self._start_session(now, window, effective_class, None, None)
            log.info("Active: started new session for %s (effective: %s)", window.app_class, effective_class)

        # Schedule delayed URL check for browser windows
        if window and self._is_browser(window):
            asyncio.create_task(self._delayed_url_check(window))

    async def on_lid_closed(self):
        """Handle laptop lid closing."""
        async with self._lock:
            self._is_lid_closed = True
            now = time.time()
            self._close_current_session(now)
            log.info("Lid closed: instantly paused tracking")

    async def on_lid_opened(self):
        """Handle laptop lid opening."""
        async with self._lock:
            self._is_lid_closed = False
            if self._is_idle:
                return  # Still idle, wait for on_active
            
            now = time.time()
            window = get_active_window()
            self._current_window = window
            if window is None:
                return

            effective_class = self._resolve_app_class(window)
            self._current_session_app_class = effective_class
            self._current_url = None
            self._current_url_title = None
            self._start_session(now, window, effective_class, None, None)
            log.info("Lid opened: resumed tracking for %s (effective: %s)", window.app_class, effective_class)

        # Schedule delayed URL check for browser windows
        if window and self._is_browser(window):
            asyncio.create_task(self._delayed_url_check(window))

    async def _delayed_url_check(self, window: WindowInfo):
        """Wait briefly for the browser extension to update, then read the URL."""
        await asyncio.sleep(0.5)
        async with self._lock:
            # Bail out if the window has changed since we scheduled this
            if self._current_window != window:
                return
            if self._is_idle or self._is_lid_closed:
                return

            url, title = read_browser_url(window.app_class)
            if url != self._current_url:
                # Update the existing session in-place (no close+reopen)
                self._current_url = url
                self._current_url_title = title
                if self._current_session_id is not None:
                    self.db.update_session_website(
                        self._current_session_id, url, title
                    )
                log.info("Browser URL resolved to: %s", url)

    async def heartbeat(self):
        """Update the current session's end_time (crash resilience)."""
        async with self._lock:
            now = time.time()
            
            # Detect sleep/suspend (if time jumped by more than 30s between heartbeats)
            if hasattr(self, '_last_time'):
                jump = now - self._last_time
                if jump > 30.0:
                    log.info("System sleep detected (time jump of %.1fs)", jump)
                    # Close the session at the last known awake time
                    if self._current_session_id is not None:
                        self._close_current_session(self._last_time)
                    # Start a fresh session for the wake-up time if not idle
                    if self._current_window and not self._is_idle:
                        self._start_session(
                            now, self._current_window, self._current_session_app_class, self._current_url, self._current_url_title
                        )

            self._last_time = now

            if self._current_session_id is not None and not self._is_idle:
                self.db.update_session_end(self._current_session_id, now)

                # Also check if browser URL has changed
                if self._current_window and self._is_browser(self._current_window):
                    new_url, new_title = read_browser_url(self._current_window.app_class)
                    if new_url != self._current_url:
                        # Website changed within browser — close old session, start new
                        self._close_current_session(now)
                        self._current_url = new_url
                        self._current_url_title = new_title
                        # Re-query Hyprland for the fresh window title
                        fresh_window = get_active_window()
                        if fresh_window:
                            self._current_window = fresh_window
                        self._start_session(
                            now, self._current_window, self._current_session_app_class, new_url, new_title
                        )
                        log.info("Browser URL changed to: %s", new_url)

    async def shutdown(self):
        """Close the current session on daemon shutdown."""
        async with self._lock:
            if self._current_session_id is not None:
                now = time.time()
                self._close_current_session(now)
                log.info("Shutdown: closed final session")

    def _start_session(
        self,
        now: float,
        window: WindowInfo,
        effective_class: str,
        url: Optional[str],
        url_title: Optional[str],
    ):
        """Start a new session in the database."""
        self._current_session_id = self.db.insert_session(
            start_time=now,
            end_time=now,
            app_class=effective_class,
            app_title=window.title,
            website_url=url,
            website_title=url_title,
        )
        log.debug(
            "Session started: id=%d app=%s url=%s",
            self._current_session_id,
            effective_class,
            url,
        )

    def _close_current_session(self, now: float):
        """Close the current session."""
        if self._current_session_id is not None:
            self.db.close_session(self._current_session_id, now)
            log.debug("Session closed: id=%d", self._current_session_id)
            self._current_session_id = None
            self._current_url = None
            self._current_url_title = None

    @staticmethod
    def _is_browser(window: WindowInfo) -> bool:
        """Check if a window is a supported browser (Chrome or Firefox)."""
        wc = window.app_class.lower()
        return CHROME_WINDOW_CLASS in wc or FIREFOX_WINDOW_CLASS in wc


# ── PID file ──────────────────────────────────────────────────────────

PID_FILE = RUNTIME_DIR / "daemon.pid"


def write_pid():
    """Write our PID to the PID file."""
    PID_FILE.write_text(str(os.getpid()))


def remove_pid():
    """Remove the PID file."""
    PID_FILE.unlink(missing_ok=True)


def is_already_running() -> bool:
    """Check if another daemon instance is already running."""
    if not PID_FILE.exists():
        return False
    try:
        pid = int(PID_FILE.read_text().strip())
        # Check if the process is actually running
        os.kill(pid, 0)
        return True
    except (ValueError, ProcessLookupError, PermissionError):
        # Stale PID file
        PID_FILE.unlink(missing_ok=True)
        return False


# ── Main daemon loop ──────────────────────────────────────────────────

async def run_daemon():
    """Main daemon entry point."""
    setup_logging()

    if is_already_running():
        log.error("Another screentime-daemon is already running. Exiting.")
        sys.exit(1)

    write_pid()
    log.info("ScreenTime daemon starting (PID %d)", os.getpid())

    db = Database()
    session_mgr = SessionManager(db)

    # Set up components
    tracker = HyprlandTracker(on_window_change=session_mgr.on_window_change)
    idle = IdleDetector(on_idle=session_mgr.on_idle, on_active=session_mgr.on_active)

    # Shutdown event
    shutdown_event = asyncio.Event()

    def handle_signal(sig):
        log.info("Received signal %s, shutting down...", sig)
        shutdown_event.set()

    loop = asyncio.get_event_loop()
    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, handle_signal, sig)

    try:
        # Start components
        await tracker.start()
        await idle.start()

        # Set initial window state
        initial_window = get_active_window()
        if initial_window:
            await session_mgr.on_window_change(initial_window)
            log.info("Initial window: %s", initial_window.app_class)

        # Heartbeat loop
        async def heartbeat_loop():
            while not shutdown_event.is_set():
                await session_mgr.heartbeat()
                try:
                    await asyncio.wait_for(
                        shutdown_event.wait(), timeout=HEARTBEAT_INTERVAL
                    )
                except asyncio.TimeoutError:
                    pass

        # Lid watcher loop (polls every 1s, highly efficient)
        async def lid_watcher_loop():
            # Find lid path dynamically
            import glob
            lid_paths = glob.glob("/proc/acpi/button/lid/*/state")
            if not lid_paths:
                log.info("No ACPI lid switch found. Lid tracking disabled.")
                return
            
            lid_path = lid_paths[0]
            was_closed = False
            
            while not shutdown_event.is_set():
                try:
                    with open(lid_path) as f:
                        state = f.read().strip()
                    is_closed = "closed" in state
                    
                    if is_closed and not was_closed:
                        await session_mgr.on_lid_closed()
                    elif not is_closed and was_closed:
                        await session_mgr.on_lid_opened()
                        
                    was_closed = is_closed
                except Exception:
                    pass
                
                try:
                    await asyncio.wait_for(shutdown_event.wait(), timeout=1.0)
                except asyncio.TimeoutError:
                    pass

        heartbeat_task = asyncio.create_task(heartbeat_loop())
        lid_task = asyncio.create_task(lid_watcher_loop())

        # Wait for shutdown signal
        await shutdown_event.wait()

        # Graceful shutdown
        heartbeat_task.cancel()
        lid_task.cancel()
        try:
            await asyncio.gather(heartbeat_task, lid_task, return_exceptions=True)
        except asyncio.CancelledError:
            pass

        await session_mgr.shutdown()
        await tracker.stop()
        await idle.stop()

        # Clean up very short sessions (< 1 second)
        db.delete_short_sessions(1.0)

    except Exception as e:
        log.exception("Daemon crashed: %s", e)
        await session_mgr.shutdown()
        raise
    finally:
        remove_pid()
        log.info("ScreenTime daemon stopped")


def main():
    """Entry point for screentime-daemon command."""
    asyncio.run(run_daemon())


if __name__ == "__main__":
    main()
