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
from typing import Callable, Awaitable, Optional

try:
    from pywayland.client import Display
    from screentime.wayland_bindings.wayland import WlOutput
    from screentime.wayland_bindings.wlr_output_power_management_unstable_v1 import ZwlrOutputPowerManagerV1, ZwlrOutputPowerV1
    WAYLAND_AVAILABLE = True
except ImportError:
    WAYLAND_AVAILABLE = False

log = logging.getLogger(__name__)


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
    but is highly efficient (~0.1ms per poll) because it connects directly
    to the Hyprland IPC socket instead of spawning subprocesses.
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
        self._task: Optional[asyncio.Task] = None
        self.is_idle = False
        self._socket_path = self._get_socket_path()

    def _get_socket_path(self) -> Optional[str]:
        """Resolve the Hyprland command socket path."""
        his = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
        if not his:
            log.warning("HYPRLAND_INSTANCE_SIGNATURE not set. Are you running Hyprland?")
            return None
            
        xdg_runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
        path = os.path.join(xdg_runtime, "hypr", his, ".socket.sock")
        
        if not os.path.exists(path):
            log.warning("Hyprland socket not found at: %s", path)
            return None
            
        return path

    async def start(self):
        if not self._socket_path:
            log.error("Cannot start Hyprland DPMS polling: socket not available")
            return
            
        self._running = True
        self._task = asyncio.create_task(self._run())
        log.info("Hyprland DPMS polling backend started (interval=%.1fs)", self._poll_interval)

    async def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        log.info("Hyprland DPMS polling backend stopped")

    async def _check_dpms(self) -> bool:
        """Query the socket for monitor status. Returns True if ANY screen is on."""
        try:
            # We use an executor because socket operations are blocking,
            # though the latency is < 1ms, it's safer for the asyncio loop.
            return await asyncio.to_thread(self._sync_check_dpms)
        except Exception as e:
            log.debug("Failed to check DPMS state: %s", e)
            # Default to active if we can't read it
            return True

    def _sync_check_dpms(self) -> bool:
        """Synchronously query the socket."""
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(1.0)
            s.connect(self._socket_path)
            s.sendall(b"j/monitors")
            
            buf = b""
            while True:
                chunk = s.recv(4096)
                if not chunk:
                    break
                buf += chunk
                
        data = json.loads(buf.decode("utf-8"))
        # If any monitor has dpmsStatus == true, the screen is active.
        return any(m.get("dpmsStatus", False) for m in data)

    async def _run(self):
        """Poll DPMS state and trigger callbacks."""
        while self._running:
            try:
                screen_active = await self._check_dpms()
                
                # State transition to IDLE (screen off)
                if not screen_active and not self.is_idle:
                    log.info("DPMS is off. User went idle.")
                    self.is_idle = True
                    await self._on_idle()
                    
                # State transition to ACTIVE (screen on)
                elif screen_active and self.is_idle:
                    log.info("DPMS is on. User became active.")
                    self.is_idle = False
                    await self._on_active()
                    
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
        on_active: Callable[[], Awaitable[None]]
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._running = False
        self.is_idle = False
        
        self.display = None
        self.registry = None
        self.power_manager = None
        self.outputs = []
        self.power_objects = []

    def _registry_global(self, registry, id_, interface, version):
        if interface == "zwlr_output_power_manager_v1":
            self.power_manager = registry.bind(id_, ZwlrOutputPowerManagerV1, version)
        elif interface == "wl_output":
            output = registry.bind(id_, WlOutput, version)
            self.outputs.append(output)

    def _on_mode(self, power, mode):
        # mode 0 = off (idle), 1 = on (active)
        is_screen_on = (mode == ZwlrOutputPowerV1.mode.on)
        
        if not is_screen_on and not self.is_idle:
            log.info("Wayland DPMS event: Screen OFF. User went idle.")
            self.is_idle = True
            asyncio.create_task(self._on_idle())
        elif is_screen_on and self.is_idle:
            log.info("Wayland DPMS event: Screen ON. User became active.")
            self.is_idle = False
            asyncio.create_task(self._on_active())

    async def start(self):
        if not WAYLAND_AVAILABLE:
            raise RuntimeError("pywayland or protocol bindings not available")
            
        self.display = Display()
        self.display.connect()
        self.registry = self.display.get_registry()
        
        # Clear previous state
        self.power_manager = None
        self.outputs.clear()
        self.power_objects.clear()
        
        self.registry.dispatcher["global"] = self._registry_global
        
        self.display.dispatch(block=True)
        self.display.roundtrip()
        
        if not self.power_manager:
            self.display.disconnect()
            raise RuntimeError("Compositor does not support zwlr_output_power_manager_v1")
            
        for output in self.outputs:
            power = self.power_manager.get_output_power(output)
            power.dispatcher["mode"] = self._on_mode
            self.power_objects.append(power)
            
        self.display.roundtrip()
        
        self._running = True
        loop = asyncio.get_running_loop()
        loop.add_reader(self.display.get_fd(), self._read_events)
        log.info("Wayland DPMS event-driven backend started (0 CPU usage)")

    def _read_events(self):
        try:
            self.display.read()
            self.display.dispatch(block=False)
        except Exception as e:
            log.error(f"Wayland read error: {e}")

    async def stop(self):
        if self._running:
            self._running = False
            loop = asyncio.get_running_loop()
            loop.remove_reader(self.display.get_fd())
            self.display.disconnect()
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
        timeout: int = None, # Unused, kept for API compatibility with old swayidle logic
    ):
        self._on_idle = on_idle
        self._on_active = on_active
        self._backend = None

    async def _init_backend(self):
        # 1. Try Event-driven Wayland Protocol
        if WAYLAND_AVAILABLE:
            try:
                backend = WlrOutputPowerBackend(self._on_idle, self._on_active)
                # Test connection and protocol support
                await backend.start()
                await backend.stop()
                self._backend = backend
                return
            except Exception as e:
                log.warning("Wayland event-driven backend unavailable: %s. Falling back...", e)
                
        # 2. Fallback to Hyprctl Polling
        self._backend = HyprctlPollingBackend(self._on_idle, self._on_active)

    @property
    def is_idle(self) -> bool:
        return self._backend.is_idle if self._backend else False

    async def start(self):
        """Start idle detection."""
        if not self._backend:
            await self._init_backend()
            
        log.info("Starting idle detector (auto-selected backend: %s)", 
                 self._backend.__class__.__name__)
        await self._backend.start()

    async def stop(self):
        """Stop idle detection."""
        await self._backend.stop()
