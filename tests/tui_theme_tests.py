import configparser
import os
import tempfile
import unittest
from pathlib import Path

from screentime.config import get_theme, save_theme
from screentime.database import Database
from screentime.tui.app import ScreenTimeApp


class TestTuiTheme(unittest.IsolatedAsyncioTestCase):
    async def test_theme_persistence_across_restart(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            db = Database(db_path=Path(tmp_dir) / "test.db")

            app1 = ScreenTimeApp(db=db, config_path=config_path)
            async with app1.run_test():
                self.assertEqual(app1.theme, "textual-dark")
                app1.theme = "nord"
                self.assertEqual(app1.theme, "nord")

            app2 = ScreenTimeApp(db=db, config_path=config_path)
            async with app2.run_test():
                self.assertEqual(
                    app2.theme,
                    "nord",
                    f"Theme should persist across restart, but was {app2.theme}",
                )

    async def test_invalid_theme_fallback(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            config_path.write_text("[UI]\ntheme = non_existent_theme\n")
            db = Database(db_path=Path(tmp_dir) / "test.db")

            app = ScreenTimeApp(db=db, config_path=config_path)
            async with app.run_test():
                self.assertEqual(app.theme, "textual-dark")

    def test_legacy_tui_section_fallback(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            config_path.write_text("[TUI]\ntheme = dracula\n")
            self.assertEqual(get_theme(config_path), "dracula")

    def test_empty_ui_theme_fallback_to_tui_and_default(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            config_path.write_text("[UI]\ntheme = \n[TUI]\ntheme = dracula\n")
            self.assertEqual(get_theme(config_path), "dracula")

            config_path.write_text("[UI]\ntheme =   \n")
            self.assertEqual(get_theme(config_path), "textual-dark")

    def test_save_theme_preserves_unrelated_sections(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            config_path.write_text("[Daemon]\nidle_timeout = 120\n[TUI]\ntheme = dracula\n")

            save_theme("nord", path=config_path)

            parser = configparser.ConfigParser()
            parser.read(config_path)
            self.assertEqual(parser.get("UI", "theme"), "nord")
            self.assertEqual(parser.get("Daemon", "idle_timeout"), "120")
            self.assertFalse(parser.has_section("TUI"))

    async def test_startup_does_not_rewrite_config(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            config_path.write_text("[UI]\ntheme = nord\n")
            mtime_before = os.path.getmtime(config_path)
            db = Database(db_path=Path(tmp_dir) / "test.db")

            app = ScreenTimeApp(db=db, config_path=config_path)
            async with app.run_test():
                self.assertEqual(app.theme, "nord")

            mtime_after = os.path.getmtime(config_path)
            self.assertEqual(
                mtime_before,
                mtime_after,
                "Starting app with existing theme should not rewrite config",
            )

    def test_theme_binding_exists(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            config_path = Path(tmp_dir) / "config.ini"
            db = Database(db_path=Path(tmp_dir) / "test.db")
            app = ScreenTimeApp(db=db, config_path=config_path)
            theme_bindings = [
                b for b in app.BINDINGS if b.action == "change_theme" and b.key == "t"
            ]
            self.assertTrue(
                len(theme_bindings) > 0, "Binding 't' for change_theme should exist"
            )


if __name__ == "__main__":
    unittest.main()
