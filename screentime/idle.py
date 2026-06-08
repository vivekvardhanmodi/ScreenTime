"""Idle detection for Hyprland using swayidle.

Launches swayidle as a subprocess to detect idle/resume transitions
using the Wayland ext-idle-notify-v1 protocol. This runs alongside
hypridle without conflicts.
"""

import asyncio
import logging
import shutil
from typing import Callable, Awaitable, Optional

from screentime import IDLE_TIMEOUT

log = logging.getLogger(__name__)


class IdleDetector:
    """Detects user idle/active transitions via swayidle.

    Calls on_idle() when user has been inactive for IDLE_TIMEOUT seconds.
    Calls on_active() when user resumes activity.
    """

    def __init__(
        self,
        on_idle: Callable[[], Awaitable[None]],
        on_active: Callable[[], Awaitable[None]],
        timeout: int = IDLE_TIMEOUT,
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._timeout = timeout
        self._process: Optional[asyncio.subprocess.Process] = None
        self._task: Optional[asyncio.Task] = None
        self._running = False
        self.is_idle = False

    async def start(self):
        """Start idle detection."""
        if not shutil.which("swayidle"):
            log.error(
                "swayidle not found! Install it: sudo pacman -S swayidle\n"
                "Idle detection will be DISABLED."
            )
            return

        self._running = True
        self._task = asyncio.create_task(self._run())
        log.info("Idle detector started (timeout=%ds)", self._timeout)

    async def stop(self):
        """Stop idle detection and kill swayidle."""
        self._running = False
        if self._process:
            try:
                self._process.terminate()
                await asyncio.wait_for(self._process.wait(), timeout=5)
            except (ProcessLookupError, asyncio.TimeoutError):
                if self._process:
                    self._process.kill()
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        log.info("Idle detector stopped")

    async def _run(self):
        """Run swayidle and monitor its output for idle/resume events."""
        while self._running:
            try:
                # Use a shell script approach: swayidle calls our commands on idle/resume
                # We create named pipes for communication
                self._process = await asyncio.create_subprocess_exec(
                    "swayidle", "-w",
                    "timeout", str(self._timeout), "echo IDLE",
                    "resume", "echo ACTIVE",
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )

                log.info("swayidle started (PID %d)", self._process.pid)

                while self._running and self._process.returncode is None:
                    try:
                        line = await asyncio.wait_for(
                            self._process.stdout.readline(),
                            timeout=5.0,
                        )
                    except asyncio.TimeoutError:
                        continue

                    if not line:
                        break

                    text = line.decode("utf-8").strip()
                    if text == "IDLE":
                        log.info("User went idle")
                        self.is_idle = True
                        await self._on_idle()
                    elif text == "ACTIVE":
                        log.info("User became active")
                        self.is_idle = False
                        await self._on_active()

                # Process ended
                if self._process.returncode is not None:
                    log.warning("swayidle exited with code %d", self._process.returncode)

            except FileNotFoundError:
                log.error("swayidle binary not found")
                return
            except Exception as e:
                log.error("Idle detector error: %s", e)

            if self._running:
                log.info("Restarting swayidle in 5 seconds...")
                await asyncio.sleep(5)
