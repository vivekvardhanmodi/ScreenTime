"""ScreenTime daemon — main orchestrator.

Coordinates window tracking, idle detection, and Chrome URL tracking
to log activity sessions into the SQLite database.

Run as: python -m screentime.daemon
Or via systemd: screentime-daemon
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
    CHROME_URL_FILE,
    CHROME_WINDOW_CLASS,
    HEARTBEAT_INTERVAL,
    RUNTIME_DIR,
)
from screentime.database import Database
from screentime.idle import IdleDetector
from screentime.tracker import HyprlandTracker, WindowInfo, get_active_window

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


# ── Chrome URL reader ─────────────────────────────────────────────────

def read_chrome_url() -> tuple[Optional[str], Optional[str]]:
    """Read the current Chrome tab URL from the state file.

    Returns (domain, title) or (None, None) if unavailable.
    """
    try:
        if not CHROME_URL_FILE.exists():
            return None, None

        with open(CHROME_URL_FILE) as f:
            data = json.load(f)

        return data.get("domain"), data.get("title")
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
        self._current_window: Optional[WindowInfo] = None
        self._current_domain: Optional[str] = None
        self._current_domain_title: Optional[str] = None
        self._is_idle = False
        self._lock = asyncio.Lock()

    async def on_window_change(self, window: Optional[WindowInfo]):
        """Handle active window change from Hyprland IPC."""
        async with self._lock:
            if self._is_idle:
                # If idle, just remember the new window for when we resume
                self._current_window = window
                return

            now = time.time()

            # Close existing session if any
            self._close_current_session(now)

            self._current_window = window

            if window is None:
                return

            # Check for Chrome URL
            domain = None
            domain_title = None
            if self._is_chrome(window):
                domain, domain_title = read_chrome_url()

            self._current_domain = domain
            self._current_domain_title = domain_title

            # Start new session
            self._start_session(now, window, domain, domain_title)

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
            now = time.time()

            # Re-query the current window since it may have changed during idle
            window = get_active_window()
            self._current_window = window

            if window is None:
                return

            domain = None
            domain_title = None
            if self._is_chrome(window):
                domain, domain_title = read_chrome_url()

            self._current_domain = domain
            self._current_domain_title = domain_title
            self._start_session(now, window, domain, domain_title)
            log.info("Active: started new session for %s", window.app_class)

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
                            now, self._current_window, self._current_domain, self._current_domain_title
                        )

            self._last_time = now

            if self._current_session_id is not None and not self._is_idle:
                self.db.update_session_end(self._current_session_id, now)

                # Also check if Chrome URL has changed
                if self._current_window and self._is_chrome(self._current_window):
                    new_domain, new_title = read_chrome_url()
                    if new_domain != self._current_domain:
                        # Website changed within Chrome — close old session, start new
                        self._close_current_session(now)
                        self._current_domain = new_domain
                        self._current_domain_title = new_title
                        self._start_session(
                            now, self._current_window, new_domain, new_title
                        )
                        log.info("Chrome URL changed to: %s", new_domain)

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
        domain: Optional[str],
        domain_title: Optional[str],
    ):
        """Start a new session in the database."""
        self._current_session_id = self.db.insert_session(
            start_time=now,
            end_time=now,
            app_class=window.app_class,
            app_title=window.title,
            website_domain=domain,
            website_title=domain_title,
        )
        log.debug(
            "Session started: id=%d app=%s domain=%s",
            self._current_session_id,
            window.app_class,
            domain,
        )

    def _close_current_session(self, now: float):
        """Close the current session."""
        if self._current_session_id is not None:
            self.db.close_session(self._current_session_id, now)
            log.debug("Session closed: id=%d", self._current_session_id)
            self._current_session_id = None
            self._current_domain = None
            self._current_domain_title = None

    @staticmethod
    def _is_chrome(window: WindowInfo) -> bool:
        """Check if a window is Google Chrome."""
        return CHROME_WINDOW_CLASS in window.app_class.lower()


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

        heartbeat_task = asyncio.create_task(heartbeat_loop())

        # Wait for shutdown signal
        await shutdown_event.wait()

        # Graceful shutdown
        heartbeat_task.cancel()
        try:
            await heartbeat_task
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
