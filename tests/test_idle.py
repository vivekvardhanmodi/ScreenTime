import asyncio
import os
import socket
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

from screentime.idle import (
    HyprctlPollingBackend,
    IdleDetector,
    WlrOutputPowerBackend,
    _parse_monitor_dpms,
)


class TestParseMonitorDpms(unittest.TestCase):
    def test_single_active_monitor(self):
        buf = b'[{"id": 0, "name": "eDP-1", "dpmsStatus": true}]'
        self.assertTrue(_parse_monitor_dpms(buf))

    def test_single_idle_monitor(self):
        buf = b'[{"id": 0, "name": "eDP-1", "dpmsStatus": false}]'
        self.assertFalse(_parse_monitor_dpms(buf))

    def test_multiple_monitors_one_active(self):
        buf = b'[{"id": 0, "dpmsStatus": false}, {"id": 1, "dpmsStatus": true}]'
        self.assertTrue(_parse_monitor_dpms(buf))

    def test_multiple_monitors_all_idle(self):
        buf = b'[{"id": 0, "dpmsStatus": false}, {"id": 1, "dpmsStatus": false}]'
        self.assertFalse(_parse_monitor_dpms(buf))

    def test_missing_dpms_status_returns_none(self):
        # A monitor missing dpmsStatus should never be inferred as idle
        buf = b'[{"id": 0, "name": "eDP-1"}]'
        self.assertIsNone(_parse_monitor_dpms(buf))

    def test_partial_missing_dpms_status_returns_none(self):
        # One monitor says false, but the other monitor is missing the key
        buf = b'[{"id": 0, "dpmsStatus": false}, {"id": 1, "name": "eDP-2"}]'
        self.assertIsNone(_parse_monitor_dpms(buf))

    def test_non_boolean_dpms_status_returns_none(self):
        buf = b'[{"id": 0, "dpmsStatus": "active"}]'
        self.assertIsNone(_parse_monitor_dpms(buf))

    def test_empty_list_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b"[]"))

    def test_malformed_html_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b"<html>not json</html>"))

    def test_truncated_bytes_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b'[{"id": 0, "dpmsStatus":'))

    def test_non_utf8_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b"\xff\xfe\x00\x00"))

    def test_non_list_json_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b'{"error": "not a list"}'))
        self.assertIsNone(_parse_monitor_dpms(b'"scalar string"'))
        self.assertIsNone(_parse_monitor_dpms(b"123"))

    def test_list_with_no_dict_items_returns_none(self):
        self.assertIsNone(_parse_monitor_dpms(b'[1, 2, "three"]'))


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
            patch("screentime.idle.Display", return_value=mock_display, create=True),
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
            patch("screentime.idle.Display", return_value=mock_display, create=True),
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

        # If on_idle raises an exception, _on_mode should swallow it safely
        self.on_idle.side_effect = RuntimeError("User callback failure")
        self.backend._on_mode(power, 0)
        await asyncio.sleep(0.01)
        self.assertTrue(self.backend.is_idle)


class TestHyprctlPollingBackend(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.on_idle = AsyncMock()
        self.on_active = AsyncMock()
        self.backend = HyprctlPollingBackend(
            self.on_idle, self.on_active, poll_interval=0.01
        )

    async def asyncTearDown(self):
        await self.backend.stop()

    async def test_start_without_socket_creates_running_task(self):
        with patch.object(self.backend, "_find_live_socket", return_value=None):
            await self.backend.start()

            self.assertTrue(self.backend._running)
            self.assertIsNotNone(self.backend._task)
            self.assertFalse(self.backend._task.done())

    async def test_start_idempotent_does_not_create_duplicate_task(self):
        with patch.object(self.backend, "_find_live_socket", return_value=None):
            await self.backend.start()
            task1 = self.backend._task
            await self.backend.start()
            task2 = self.backend._task
            self.assertIs(task1, task2)

    async def test_unavailable_socket_with_idle_true_does_not_emit_on_active(self):
        self.backend.is_idle = True

        with patch.object(self.backend, "_check_dpms", return_value=None):
            await self.backend.start()
            await asyncio.sleep(0.03)

            self.on_active.assert_not_called()
            self.assertTrue(self.backend.is_idle)

    async def test_stale_env_socket_falls_back_to_live_socket(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            hypr_dir = os.path.join(tmpdir, "hypr")
            stale_dir = os.path.join(hypr_dir, "stale_instance")
            new_dir = os.path.join(hypr_dir, "new_instance")
            os.makedirs(stale_dir)
            os.makedirs(new_dir)

            stale_sock = os.path.join(stale_dir, ".socket.sock")
            new_sock = os.path.join(new_dir, ".socket.sock")

            Path(stale_sock).touch()
            Path(new_sock).touch()

            os.utime(stale_sock, (1000, 1000))
            os.utime(new_sock, (2000, 2000))

            env = {
                "XDG_RUNTIME_DIR": tmpdir,
                "HYPRLAND_INSTANCE_SIGNATURE": "stale_instance",
            }

            def mock_query(path, timeout=1.0, max_bytes=65536):
                if path == stale_sock:
                    raise ConnectionRefusedError("Connection refused on stale socket")
                if path == new_sock:
                    return True
                raise FileNotFoundError(path)

            with (
                patch.dict(os.environ, env, clear=False),
                patch.object(self.backend, "_query_socket", side_effect=mock_query),
            ):
                result = self.backend._sync_check_dpms()

                self.assertTrue(result)
                self.assertEqual(self.backend._socket_path, new_sock)

    async def test_find_live_socket_skips_malformed_and_finds_live(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            hypr_dir = os.path.join(tmpdir, "hypr")
            bad_dir = os.path.join(hypr_dir, "bad_instance")
            good_dir = os.path.join(hypr_dir, "good_instance")
            os.makedirs(bad_dir)
            os.makedirs(good_dir)

            bad_sock = os.path.join(bad_dir, ".socket.sock")
            good_sock = os.path.join(good_dir, ".socket.sock")

            Path(bad_sock).touch()
            Path(good_sock).touch()

            os.utime(good_sock, (1000, 1000))
            os.utime(bad_sock, (2000, 2000))

            env = {
                "XDG_RUNTIME_DIR": tmpdir,
                "HYPRLAND_INSTANCE_SIGNATURE": "",
            }

            def mock_query(path, timeout=1.0, max_bytes=65536):
                if path == bad_sock:
                    raise OSError("Corrupted socket")
                if path == good_sock:
                    return True
                return None

            with (
                patch.dict(os.environ, env, clear=False),
                patch.object(self.backend, "_query_socket", side_effect=mock_query),
            ):
                live = self.backend._find_live_socket()
                self.assertIsNotNone(live)
                path, state = live
                self.assertEqual(path, good_sock)
                self.assertTrue(state)

    async def test_sync_check_dpms_retains_socket_on_parse_failure(self):
        self.backend._socket_path = "/valid/path.sock"
        with patch.object(self.backend, "_query_socket", return_value=None):
            result = self.backend._sync_check_dpms()
            self.assertIsNone(result)
            # Socket path must NOT be invalidated when query succeeds without OSError
            self.assertEqual(self.backend._socket_path, "/valid/path.sock")

    async def test_stop_self_task_does_not_self_await(self):
        # Verify stop() called from within the polling task does not self-await / dead-lock
        self.backend._running = True
        current_task = asyncio.current_task()
        self.backend._task = current_task

        await self.backend.stop()
        self.assertFalse(self.backend._running)
        self.assertIsNone(self.backend._task)


class TestIdleDetector(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.on_idle = AsyncMock()
        self.on_active = AsyncMock()
        self.detector = IdleDetector(self.on_idle, self.on_active)

    async def asyncTearDown(self):
        await self.detector.stop()

    async def test_start_idempotent(self):
        with (
            patch.object(
                WlrOutputPowerBackend, "start", AsyncMock()
            ) as mock_backend_start,
            patch("screentime.idle.WAYLAND_AVAILABLE", True),
        ):
            await self.detector.start()
            backend = self.detector._backend
            await self.detector.start()
            self.assertIs(self.detector._backend, backend)
            self.assertEqual(mock_backend_start.await_count, 1)

    async def test_start_failure_rolls_back_running_state(self):
        with (
            patch("screentime.idle.WAYLAND_AVAILABLE", False),
            patch.object(
                HyprctlPollingBackend,
                "start",
                side_effect=RuntimeError("Cannot start polling"),
            ),
        ):
            with self.assertRaises(RuntimeError):
                await self.detector.start()

            self.assertFalse(self.detector._running)
            self.assertIsNone(self.detector._backend)

    async def test_stop_try_finally_clears_backend_on_stop_error(self):
        mock_backend = MagicMock(spec=HyprctlPollingBackend)
        mock_backend.stop = AsyncMock(side_effect=RuntimeError("Teardown error"))
        self.detector._backend = mock_backend
        self.detector._running = True

        with self.assertRaises(RuntimeError):
            await self.detector.stop()

        self.assertFalse(self.detector._running)
        self.assertIsNone(self.detector._backend)

    async def test_fallback_to_hyprctl_on_startup_failure(self):
        with (
            patch("screentime.idle.WAYLAND_AVAILABLE", True),
            patch(
                "screentime.idle.WlrOutputPowerBackend.start",
                side_effect=RuntimeError("Protocol unsupported"),
            ),
            patch.object(HyprctlPollingBackend, "_check_dpms", return_value=True),
        ):
            await self.detector.start()

            self.assertIsInstance(self.detector._backend, HyprctlPollingBackend)
            self.assertTrue(self.detector._backend._running)

    async def test_fallback_preserves_idle_true(self):
        self.detector._running = True
        mock_backend = MagicMock(spec=WlrOutputPowerBackend)
        mock_backend.is_idle = True
        mock_backend.stop = AsyncMock()
        self.detector._backend = mock_backend

        with patch.object(HyprctlPollingBackend, "_check_dpms", return_value=None):
            await self.detector._switch_to_polling(already_stopped=True)

            self.assertIsInstance(self.detector._backend, HyprctlPollingBackend)
            self.assertTrue(self.detector.is_idle)
            self.assertTrue(self.detector._backend.is_idle)
            mock_backend.stop.assert_not_called()

    async def test_switch_to_polling_direct_call_stops_previous_poller(self):
        self.detector._running = True
        old_poller = MagicMock(spec=HyprctlPollingBackend)
        old_poller.is_idle = False
        old_poller.stop = AsyncMock()
        self.detector._backend = old_poller

        with patch.object(HyprctlPollingBackend, "_check_dpms", return_value=True):
            # Calling _switch_to_polling directly without already_stopped must stop the old poller
            await self.detector._switch_to_polling()

            old_poller.stop.assert_awaited_once()
            self.assertIsInstance(self.detector._backend, HyprctlPollingBackend)
            self.assertIsNot(self.detector._backend, old_poller)

    async def test_switch_to_polling_when_not_running_is_noop(self):
        self.detector._running = False
        self.detector._backend = None

        await self.detector._switch_to_polling()
        self.assertIsNone(self.detector._backend)

    async def test_end_to_end_read_events_failure_switches_detector_to_polling_and_preserves_idle(
        self,
    ):
        sock_r, sock_w = socket.socketpair()
        self.addCleanup(sock_r.close)
        self.addCleanup(sock_w.close)

        mock_display = MagicMock()
        mock_display.get_fd.return_value = sock_r.fileno()
        mock_display.read.side_effect = RuntimeError("Failed to read events")

        def mock_connect(backend_self):
            backend_self.display = mock_display
            backend_self._fd = sock_r.fileno()
            backend_self.power_manager = MagicMock()

        with (
            patch.object(
                WlrOutputPowerBackend,
                "_connect",
                autospec=True,
                side_effect=mock_connect,
            ),
            patch("screentime.idle.WAYLAND_AVAILABLE", True),
            patch.object(HyprctlPollingBackend, "_check_dpms", return_value=None),
        ):
            await self.detector.start()
            self.assertIsInstance(self.detector._backend, WlrOutputPowerBackend)
            wlr_backend = self.detector._backend

            # Set user as idle prior to disconnect
            wlr_backend.is_idle = True
            self.assertTrue(self.detector.is_idle)

            # Trigger real read failure in WlrOutputPowerBackend
            wlr_backend._read_events()

            # Allow the disconnect task to run and complete the switch
            await asyncio.sleep(0.05)

            # Detector must have switched to HyprctlPollingBackend and preserved idle state
            self.assertIsInstance(self.detector._backend, HyprctlPollingBackend)
            self.assertTrue(self.detector._backend._running)
            self.assertTrue(self.detector._backend.is_idle)
            self.assertTrue(self.detector.is_idle)
            self.assertFalse(wlr_backend._running)
            self.assertIsNone(wlr_backend.display)


if __name__ == "__main__":
    unittest.main()
