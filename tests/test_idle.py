import asyncio
import unittest
from unittest.mock import AsyncMock, MagicMock, patch

from screentime.idle import WlrOutputPowerBackend


class TestWlrOutputPowerBackend(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.on_idle = AsyncMock()
        self.on_active = AsyncMock()
        self.on_disconnect = AsyncMock()
        self.backend = WlrOutputPowerBackend(
            self.on_idle, self.on_active, on_disconnect=self.on_disconnect
        )

    async def asyncTearDown(self):
        await self.backend.stop()

    async def test_read_events_read_exception_unregisters_reader_and_cleans_up(self):
        loop = asyncio.get_running_loop()
        self.backend._loop = loop
        self.backend._running = True
        self.backend._fd = 42

        mock_display = MagicMock()
        mock_display.read.side_effect = RuntimeError("Failed to read events")
        self.backend.display = mock_display

        with patch.object(loop, "remove_reader") as mock_remove_reader:
            self.backend._read_events()

            mock_remove_reader.assert_called_once_with(42)
            mock_display.disconnect.assert_called_once()
            self.assertIsNone(self.backend.display)
            self.assertIsNone(self.backend._fd)
            self.assertFalse(self.backend._running)

        await asyncio.sleep(0.01)
        self.on_disconnect.assert_awaited_once()

    async def test_read_events_dispatch_exception_unregisters_reader_and_cleans_up(
        self,
    ):
        loop = asyncio.get_running_loop()
        self.backend._loop = loop
        self.backend._running = True
        self.backend._fd = 42

        mock_display = MagicMock()
        mock_display.dispatch.side_effect = RuntimeError(
            "Wayland connection severed in dispatch"
        )
        self.backend.display = mock_display

        with patch.object(loop, "remove_reader") as mock_remove_reader:
            self.backend._read_events()

            # Dispatch failure must be treated as fatal to the Wayland session
            mock_remove_reader.assert_called_once_with(42)
            mock_display.disconnect.assert_called_once()
            self.assertIsNone(self.backend.display)
            self.assertIsNone(self.backend._fd)
            self.assertFalse(self.backend._running)

        await asyncio.sleep(0.01)
        self.on_disconnect.assert_awaited_once()

    async def test_read_events_noop_when_display_is_none(self):
        loop = asyncio.get_running_loop()
        self.backend._loop = loop
        self.backend.display = None

        with patch.object(loop, "remove_reader") as mock_remove_reader:
            self.backend._read_events()
            mock_remove_reader.assert_not_called()

    async def test_connect_partial_handshake_failure_disconnects_display(self):
        mock_display = MagicMock()
        mock_display.get_registry.side_effect = RuntimeError(
            "Compositor handshake failed"
        )

        with (
            patch("screentime.idle.Display", return_value=mock_display),
            patch("screentime.idle.WAYLAND_AVAILABLE", True),
        ):
            with self.assertRaises(RuntimeError):
                self.backend._connect()

            mock_display.disconnect.assert_called_once()
            self.assertIsNone(self.backend.display)
            self.assertIsNone(self.backend._fd)

    async def test_add_reader_failure_cleans_up_display(self):
        loop = asyncio.get_running_loop()
        mock_display = MagicMock()
        mock_display.get_fd.return_value = 101

        with (
            patch("screentime.idle.Display", return_value=mock_display),
            patch("screentime.idle.WAYLAND_AVAILABLE", True),
            patch.object(
                loop, "add_reader", side_effect=RuntimeError("add_reader failed")
            ),
        ):
            with self.assertRaises(RuntimeError):
                await self.backend.start()

            mock_display.disconnect.assert_called_once()
            self.assertIsNone(self.backend.display)
            self.assertIsNone(self.backend._fd)
            self.assertFalse(self.backend._running)

    async def test_stop_unregisters_reader_and_disconnects(self):
        loop = asyncio.get_running_loop()
        self.backend._loop = loop
        self.backend._running = True
        self.backend._fd = 55
        mock_display = MagicMock()
        self.backend.display = mock_display

        with patch.object(loop, "remove_reader") as mock_remove_reader:
            await self.backend.stop()

            mock_remove_reader.assert_called_once_with(55)
            mock_display.disconnect.assert_called_once()
            self.assertIsNone(self.backend.display)
            self.assertIsNone(self.backend._fd)
            self.assertFalse(self.backend._running)

    async def test_start_when_already_running_is_noop(self):
        self.backend._running = True
        with patch.object(self.backend, "_connect") as mock_connect:
            await self.backend.start()
            mock_connect.assert_not_called()

    async def test_multi_monitor_dpms_tracking(self):
        power_a = MagicMock()
        power_b = MagicMock()

        self.backend._output_states[power_a] = True
        self.backend._output_states[power_b] = True
        self.backend.is_idle = False

        # Monitor A turns OFF (mode 0) -> Monitor B is still ON, so system remains active
        self.backend._on_mode(power_a, 0)
        self.assertFalse(self.backend.is_idle)
        self.on_idle.assert_not_called()

        # Monitor B turns OFF (mode 0) -> Both monitors are now OFF, so system becomes idle
        self.backend._on_mode(power_b, 0)
        self.assertTrue(self.backend.is_idle)
        await asyncio.sleep(0.01)
        self.on_idle.assert_awaited_once()

        # Monitor A turns ON (mode 1) -> At least one monitor is ON, so system becomes active
        self.backend._on_mode(power_a, 1)
        self.assertFalse(self.backend.is_idle)
        await asyncio.sleep(0.01)
        self.on_active.assert_awaited_once()

    async def test_on_mode_callback_exception_does_not_raise(self):
        power = MagicMock()
        self.backend._output_states[power] = True
        self.backend.is_idle = False

        self.on_idle.side_effect = RuntimeError("User callback failure")
        self.backend._on_mode(power, 0)
        await asyncio.sleep(0.01)
        self.assertTrue(self.backend.is_idle)


if __name__ == "__main__":
    unittest.main()
