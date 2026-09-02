"""ScreenTime TUI — Main application.

Rich terminal interface for viewing screen time statistics,
managing categories, and exporting data.
"""

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import (
    Footer,
    Header,
    TabbedContent,
    TabPane,
)

from screentime.database import Database
from screentime.tui.dashboard import DashboardPane
from screentime.tui.history import DailyPane, WeeklyPane, MonthlyPane
from screentime.tui.categories import CategoriesPane
from screentime.tui.groups import GroupsPane
from screentime.tui.rules import RulesPane
from screentime.tui.export import ExportPane


class ScreenTimeApp(App):
    """ScreenTime terminal UI application."""

    TITLE = "ScreenTime"
    SUB_TITLE = "App Usage Tracker"

    CSS = """
    Screen {
        background: $surface;
    }

    Header {
        dock: top;
        background: $primary;
        color: $text;
    }

    TabbedContent {
        height: 1fr;
    }

    TabPane {
        padding: 1 2;
    }

    /* ── Common stat styles ─────────────────────────── */

    .total-time-box {
        height: 5;
        width: 100%;
        background: $primary 15%;
        border: tall $primary;
        content-align: center middle;
        text-align: center;
        margin-bottom: 1;
    }

    .total-time-label {
        color: $text-muted;
        text-style: italic;
    }

    .total-time-value {
        color: $text;
        text-style: bold;
        text-align: center;
    }

    .section-title {
        color: $accent;
        text-style: bold;
        margin: 1 0 0 0;
        padding: 0 0;
    }

    /* ── App usage row ──────────────────────────────── */

    .usage-row {
        height: 3;
        margin: 0;
        padding: 0 1;
    }

    .usage-row:hover {
        background: $primary 10%;
    }

    .app-name {
        width: 30;
        padding: 1 1;
    }

    .app-category {
        width: 18;
        padding: 1 0;
        color: $text-muted;
    }

    .app-time {
        width: 12;
        padding: 1 0;
        text-align: right;
        color: $accent;
        text-style: bold;
    }

    .app-bar-container {
        width: 1fr;
        padding: 1 1;
    }

    .app-bar {
        height: 1;
        background: $primary 30%;
    }

    .app-bar-fill {
        height: 1;
    }

    /* ── Navigation ─────────────────────────────────── */

    .nav-bar {
        height: 3;
        align: center middle;
        margin-bottom: 1;
    }

    .nav-button {
        min-width: 8;
        margin: 0 1;
    }

    .date-label {
        padding: 1 2;
        text-align: center;
        text-style: bold;
        color: $text;
        width: 30;
    }

    /* ── Empty state ────────────────────────────────── */

    .empty-state {
        height: 100%;
        width: 100%;
        content-align: center middle;
        text-align: center;
        color: $text-muted;
    }

    /* ── Categories ─────────────────────────────────── */

    .category-list {
        height: 1fr;
        border: tall $primary;
        padding: 1;
    }

    .category-item {
        height: 3;
        padding: 1 2;
    }

    .category-item:hover {
        background: $primary 10%;
    }

    /* ── Input dialog ───────────────────────────────── */

    .dialog-overlay {
        align: center middle;
        background: $surface 80%;
    }

    .dialog-box {
        width: 60;
        height: auto;
        max-height: 80%;
        background: $surface;
        border: thick $primary;
        padding: 1 2;
    }

    .dialog-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    /* ── Export ──────────────────────────────────────── */

    .export-box {
        height: auto;
        width: 100%;
        background: $primary 10%;
        border: tall $primary;
        padding: 2 4;
        margin: 2 0;
    }

    .export-success {
        color: $success;
        text-style: bold;
    }

    .export-info {
        color: $text-muted;
        margin-top: 1;
    }
    """

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("r", "refresh", "Refresh"),
        Binding("d", "show_tab('tab-today')", "Today", show=False),
    ]

    def __init__(self):
        super().__init__()
        self.db = Database()

    def compose(self) -> ComposeResult:
        yield Header()
        with TabbedContent(id="tabs"):
            with TabPane("Today", id="tab-today"):
                yield DashboardPane(self.db)
            with TabPane("Daily", id="tab-daily"):
                yield DailyPane(self.db)
            with TabPane("Weekly", id="tab-weekly"):
                yield WeeklyPane(self.db)
            with TabPane("Monthly", id="tab-monthly"):
                yield MonthlyPane(self.db)
            with TabPane("Groups", id="tab-groups"):
                yield GroupsPane(self.db)
            with TabPane("Title Rules", id="tab-rules"):
                yield RulesPane(self.db)
            with TabPane("Categories", id="tab-categories"):
                yield CategoriesPane(self.db)
            with TabPane("Export", id="tab-export"):
                yield ExportPane(self.db)
        yield Footer()

    def action_refresh(self):
        """Refresh all panes."""
        for pane in self.query(DashboardPane):
            pane.refresh_data()
        for pane in self.query(DailyPane):
            pane.refresh_data()
        for pane in self.query(WeeklyPane):
            pane.refresh_data()
        for pane in self.query(MonthlyPane):
            pane.refresh_data()
        for pane in self.query(GroupsPane):
            pane.refresh_data()
        for pane in self.query(CategoriesPane):
            pane.refresh_data()


def main():
    """Entry point for the screentime TUI command."""
    app = ScreenTimeApp()
    app.run()


if __name__ == "__main__":
    main()
