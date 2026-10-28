"""Idle detection for Linux compositors.

Detects user idle/active transitions by tracking the physical screen state (DPMS).
This delegates all inhibitor logic (e.g., VLC playing, D-Bus ScreenSaver calls)
to the user's native idle manager (like hypridle, swayidle, or KDE/GNOME native),
which ultimately controls the screen state.
"""

import asyncio
import json
import logging
import os
import socket
from collections.abc import Awaitable, Callable
from typing import Any

try:
    from pywayland.client import Display

    from screentime.wayland_bindings.wayland import WlOutput
    from screentime.wayland_bindings.wlr_output_power_management_unstable_v1 import (
        ZwlrOutputPowerManagerV1,
        ZwlrOutputPowerV1,
    )

    WAYLAND_AVAILABLE = True
except ImportError:
    Display = None  # type: ignore[assignment,misc]
    WlOutput = None  # type: ignore[assignment,misc]
    ZwlrOutputPowerManagerV1 = None  # type: ignore[assignment,misc]
    ZwlrOutputPowerV1 = None  # type: ignore[assignment,misc]
    WAYLAND_AVAILABLE = False

log = logging.getLogger(__name__)


def _parse_monitor_dpms(buf: bytes) -> bool | None:
    """Parse Hyprland JSON monitor response and determine DPMS state.

    Returns:
        True: at least one monitor is confirmed on (active).
        False: all monitors are confirmed off via valid dpmsStatus=False (idle).
        None: response is empty, malformed, or contains monitors without a boolean dpmsStatus.
    """
    try:
        text = buf.decode("utf-8")
        data = json.loads(text)
        if not isinstance(data, list) or len(data) == 0:
            return None

        all_off = True
        for m in data:
            if not isinstance(m, dict) or not isinstance(m.get("dpmsStatus"), bool):
                # Monitor entry is invalid or missing boolean dpmsStatus
                all_off = False
                continue
            if m["dpmsStatus"]:
                return True

        return False if all_off else None
    except (UnicodeDecodeError, ValueError, TypeError, AttributeError):
        return None


class IdleBackend:
    """Base class for idle detection backends."""

    is_idle: bool = False

    async def start(self):
        raise NotImplementedError

    async def stop(self):
        raise NotImplementedError


class HyprctlPollingBackend(IdleBackend):
    """Detects idle state by polling Hyprland's dpmsStatus via its Unix socket.

    This is a fallback for when the Wayland protocol cannot be used directly,
    connecting directly to the Hyprland IPC socket.
    """

    def __init__(
        self,
        on_idle: Callable[[], Awaitable[None]],
        on_active: Callable[[], Awaitable[None]],
        poll_interval: float = 2.0,
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._poll_interval = poll_interval
        self._running = False
        self._task: asyncio.Task | None = None
        self.is_idle = False
        self._socket_path: str | None = None

    def _query_socket(
        self, path: str, timeout: float = 1.0, max_bytes: int = 65536
    ) -> bool | None:
        """Query a socket path for monitor DPMS state with bounded size and timeout.

        Returns:
            True / False on valid query, or None if response is unparseable.
        """
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(timeout)
            s.connect(path)
            s.sendall(b"j/monitors")

            buf = b""
            while len(buf) < max_bytes:
                chunk = s.recv(4096)
                if not chunk:
                    break
                buf += chunk

        return _parse_monitor_dpms(buf)

    def _find_live_socket(self) -> tuple[str, bool | None] | None:
        """Discover and return a responsive Hyprland command socket and its initial state.

        Prefers the env-specified signature first. If absent or unresponsive,
        probes other active sockets in newest-first order.
        """
        xdg_runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
        dirs_to_check = [os.path.join(xdg_runtime, "hypr"), "/tmp/hypr"]

        # 1. Prefer HYPRLAND_INSTANCE_SIGNATURE if set
        his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
        if his:
            for base in dirs_to_check:
                path = os.path.join(base, his, ".socket.sock")
                if os.path.exists(path):
                    try:
                        state = self._query_socket(path, timeout=0.5)
                        return path, state
                    except (OSError, socket.timeout):
                        pass

        # 2. Discover other candidates sorted newest-first
        candidates: list[tuple[float, str]] = []
        for base in dirs_to_check:
            if not os.path.isdir(base):
                continue
            try:
                for entry in os.listdir(base):
                    sock = os.path.join(base, entry, ".socket.sock")
                    if os.path.exists(sock):
                        try:
                            mtime = os.path.getmtime(sock)
                            candidates.append((mtime, sock))
                        except OSError:
                            pass
            except OSError:
                pass

        if not candidates:
            return None

        candidates.sort(key=lambda x: x[0], reverse=True)
        seen: set[str] = set()
        for _, path in candidates:
            if path in seen:
                continue
            seen.add(path)
            try:
                state = self._query_socket(path, timeout=0.5)
                return path, state
            except (OSError, socket.timeout):
                continue

        return None

    def _sync_check_dpms(self) -> bool | None:
        """Synchronously query the socket for monitor DPMS status.

        Returns:
            True if any monitor is on.
            False if all monitors are off.
            None if the status cannot be determined.
        """
        if self._socket_path:
            try:
                return self._query_socket(self._socket_path)
            except (OSError, socket.timeout) as e:
                log.debug(
                    "DPMS query connection failed on %s: %s", self._socket_path, e
                )
                self._socket_path = None

        # Socket was unset or connection failed; discover a live socket
        live = self._find_live_socket()
        if not live:
            return None

        path, state = live
        self._socket_path = path
        return state

    async def _check_dpms(self) -> bool | None:
        """Query the socket for monitor status."""
        try:
            return await asyncio.to_thread(self._sync_check_dpms)
        except Exception as e:
            log.debug("Failed to check DPMS state: %s", e)
            return None

    async def start(self):
        """Start polling Hyprland DPMS state."""
        if self._running:
            return
        self._running = True
        self._task = asyncio.create_task(self._run())
        log.info(
            "Hyprland DPMS polling backend started (interval=%.1fs)",
            self._poll_interval,
        )

    async def stop(self):
        self._running = False
        current_task = asyncio.current_task()
        if self._task and self._task is not current_task and not self._task.done():
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        self._task = None
        log.info("Hyprland DPMS polling backend stopped")

    async def _run(self):
        """Poll DPMS state and trigger callbacks."""
        while self._running:
            try:
                screen_active = await self._check_dpms()

                # State transition to IDLE (screen off)
                if screen_active is False and not self.is_idle:
                    log.info("DPMS is off. User went idle.")
                    self.is_idle = True
                    await self._on_idle()

                # State transition to ACTIVE (screen on)
                elif screen_active is True and self.is_idle:
                    log.info("DPMS is on. User became active.")
                    self.is_idle = False
                    await self._on_active()

                # If screen_active is None: compositor is unavailable or state unknown.
                # Maintain existing is_idle state; do not trigger transitions.

            except asyncio.CancelledError:
                break
            except Exception as e:
                log.error("Idle detector error: %s", e)

            if self._running:
                await asyncio.sleep(self._poll_interval)


class WlrOutputPowerBackend(IdleBackend):
    """Detects idle state via zwlr_output_power_manager_v1 Wayland protocol.

    This is the ideal event-driven backend for wlroots compositors.
    It connects to the Wayland socket, listens for DPMS state changes,
    and runs asynchronously using asyncio file descriptor polling.
    """

    def __init__(
        self,
        on_idle: Callable[[], Awaitable[None]],
        on_active: Callable[[], Awaitable[None]],
        on_disconnect: Callable[[], Awaitable[None]] | None = None,
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._on_disconnect = on_disconnect
        self._running = False
        self.is_idle = False

        self.display: Display | None = None
        self.registry = None
        self.power_manager = None
        self.outputs = []
        self.power_objects = []
        self._output_states: dict[Any, bool] = {}

        self._loop: asyncio.AbstractEventLoop | None = None
        self._fd: int | None = None
        self._disconnect_task: asyncio.Task | None = None

    def _registry_global(self, registry, id_, interface, version):
        if interface == "zwlr_output_power_manager_v1":
            self.power_manager = registry.bind(id_, ZwlrOutputPowerManagerV1, version)
        elif interface == "wl_output":
            output = registry.bind(id_, WlOutput, version)
            self.outputs.append(output)

    def _trigger_callback(self, coro: Awaitable[None], name: str):
        async def _wrapper():
            try:
                await coro
            except Exception as e:
                log.error("Unhandled exception in %s callback: %s", name, e)

        return asyncio.create_task(_wrapper())

    def _on_mode(self, power, mode):
        try:
            is_screen_on = (
                mode == ZwlrOutputPowerV1.mode.on if ZwlrOutputPowerV1 else mode == 1
            )
            self._output_states[power] = is_screen_on
            any_screen_on = any(self._output_states.values())

            if not any_screen_on and not self.is_idle:
                log.info("Wayland DPMS event: All screens OFF. User went idle.")
                self.is_idle = True
                self._trigger_callback(self._on_idle(), "on_idle")
            elif any_screen_on and self.is_idle:
                log.info("Wayland DPMS event: Screen ON. User became active.")
                self.is_idle = False
                self._trigger_callback(self._on_active(), "on_active")
        except Exception as e:
            log.error("Error handling Wayland DPMS mode event: %s", e)

    def _connect(self):
        """Establish connection to Wayland display and bind power objects."""
        if not WAYLAND_AVAILABLE:
            raise RuntimeError("pywayland or protocol bindings not available")

        display = Display()
        try:
            display.connect()
            registry = display.get_registry()

            # Clear previous state
            self.power_manager = None
            self.outputs.clear()
            self.power_objects.clear()
            self._output_states.clear()

            registry.dispatcher["global"] = self._registry_global

            display.dispatch(block=True)
            display.roundtrip()

            if not self.power_manager:
                raise RuntimeError(
                    "Compositor does not support zwlr_output_power_manager_v1"
                )

            for output in self.outputs:
                power = self.power_manager.get_output_power(output)
                self._output_states[power] = True
                power.dispatcher["mode"] = self._on_mode
                self.power_objects.append(power)

            display.roundtrip()
            self.display = display
            self.registry = registry
            self._fd = display.get_fd()
        except Exception:
            try:
                display.disconnect()
            except Exception:
                pass
            self._cleanup_display()
            raise

    def _cleanup_display(self):
        """Safely unregister the file descriptor and disconnect the display."""
        if self._loop and self._fd is not None:
            try:
                self._loop.remove_reader(self._fd)
            except Exception:
                pass
            self._fd = None

        if self.display:
            try:
                self.display.disconnect()
            except Exception:
                pass
            self.display = None

        self.registry = None
        self.power_manager = None
        self.outputs.clear()
        self.power_objects.clear()
        self._output_states.clear()
        self._running = False

    async def start(self):
        if self._running:
            return
        self._loop = asyncio.get_running_loop()
        self._connect()
        try:
            self._loop.add_reader(self._fd, self._read_events)
            self._running = True
            log.info("Wayland DPMS event-driven backend started (0 CPU usage)")
        except Exception:
            self._cleanup_display()
            raise

    def _read_events(self):
        if self.display is None:
            return
        try:
            self.display.read()
            self.display.dispatch(block=False)
        except Exception as e:
            log.warning("Wayland display read/dispatch error: %s. Cleaning up...", e)
            self._handle_disconnect()

    def _handle_disconnect(self):
        """Stop polling immediately to avoid 100% CPU spin and notify lifecycle manager."""
        self._cleanup_display()
        if self._on_disconnect:
            # Retain task reference so it is not garbage collected mid-execution
            self._disconnect_task = asyncio.create_task(self._on_disconnect())

    async def stop(self):
        self._cleanup_display()
        current_task = asyncio.current_task()
        if (
            self._disconnect_task
            and self._disconnect_task is not current_task
            and not self._disconnect_task.done()
        ):
            self._disconnect_task.cancel()
            try:
                await self._disconnect_task
            except asyncio.CancelledError:
                pass
        self._disconnect_task = None
        log.info("Wayland DPMS event-driven backend stopped")


class DBusIdleBackend(IdleBackend):
    """Detects idle state via GNOME/KDE ScreenSaver D-Bus signals.

    This provides event-driven idle detection for non-wlroots compositors.
    """

    def __init__(self, on_idle, on_active):
        raise NotImplementedError("D-Bus idle backend not yet implemented.")


class IdleDetector:
    """Detects user idle/active transitions across different compositors.

    Abstracts away the underlying implementation details (Wayland, D-Bus, polling).
    """

    def __init__(
        self,
        on_idle: Callable[[], Awaitable[None]],
        on_active: Callable[[], Awaitable[None]],
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._backend: IdleBackend | None = None
        self._running: bool = False
        self._lock = asyncio.Lock()

    @property
    def is_idle(self) -> bool:
        return self._backend.is_idle if self._backend else False

    async def _on_wayland_disconnect(self):
        """Handle runtime Wayland disconnection by falling back to polling."""
        if not self._running:
            return
        log.warning(
            "Wayland DPMS backend disconnected at runtime. Falling back to Hyprctl polling..."
        )
        await self._switch_to_polling(already_stopped=True)

    async def _switch_to_polling(self, already_stopped: bool = False):
        """Switch idle detection to Hyprctl polling backend.

        Args:
            already_stopped: Set to True if the caller already tore down the previous backend
                             (e.g. WlrOutputPowerBackend on disconnect), avoiding redundant teardown.
        """
        async with self._lock:
            if not self._running:
                return

            prev_is_idle = self.is_idle
            if self._backend and not already_stopped:
                try:
                    await self._backend.stop()
                except Exception as e:
                    log.warning("Error stopping previous backend: %s", e)

            backend = HyprctlPollingBackend(self._on_idle, self._on_active)
            backend.is_idle = prev_is_idle
            self._backend = backend
            await self._backend.start()
            log.info(
                "Active idle backend switched to: %s",
                self._backend.__class__.__name__,
            )

    async def start(self):
        """Start idle detection with the best available backend."""
        async with self._lock:
            if self._running:
                return
            self._running = True

            try:
                # 1. Try Event-driven Wayland Protocol
                if WAYLAND_AVAILABLE:
                    try:
                        backend = WlrOutputPowerBackend(
                            self._on_idle,
                            self._on_active,
                            on_disconnect=self._on_wayland_disconnect,
                        )
                        await backend.start()
                        self._backend = backend
                        log.info(
                            "Starting idle detector (auto-selected backend: %s)",
                            self._backend.__class__.__name__,
                        )
                        return
                    except Exception as e:
                        log.warning(
                            "Wayland event-driven backend unavailable: %s. Falling back to Hyprctl polling...",
                            e,
                        )

                # 2. Fallback to Hyprctl Polling
                backend = HyprctlPollingBackend(self._on_idle, self._on_active)
                await backend.start()
                self._backend = backend
                log.info(
                    "Starting idle detector (auto-selected backend: %s)",
                    self._backend.__class__.__name__,
                )
            except Exception:
                # Roll back state if both backends fail to start
                self._running = False
                if self._backend:
                    try:
                        await self._backend.stop()
                    finally:
                        self._backend = None
                raise

    async def stop(self):
        """Stop idle detection."""
        async with self._lock:
            self._running = False
            if self._backend:
                try:
                    await self._backend.stop()
                finally:
                    self._backend = None
