"""Export pane — CSV data export."""

from datetime import datetime
from pathlib import Path

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.widget import Widget
from textual.widgets import Button, Input, Label, Static

from screentime.database import Database


class ExportPane(Widget):
    """CSV export pane."""

    DEFAULT_CSS = """
    ExportPane {
        height: 1fr;
        width: 100%;
        padding: 1;
    }

    ExportPane .export-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    ExportPane .export-description {
        color: $text-muted;
        margin-bottom: 2;
    }

    ExportPane .export-path-label {
        margin-bottom: 0;
    }

    ExportPane .export-path-input {
        margin-bottom: 1;
    }

    ExportPane .export-btn {
        margin-top: 1;
        min-width: 20;
    }

    ExportPane .export-result {
        margin-top: 2;
        padding: 1 2;
        border: tall $primary;
        background: $primary 10%;
    }

    ExportPane .export-success {
        color: $success;
        text-style: bold;
    }

    ExportPane .export-error {
        color: $error;
        text-style: bold;
    }

    ExportPane .export-info {
        color: $text-muted;
        margin-top: 1;
    }

    ExportPane .stats-summary {
        margin-top: 2;
        padding: 1 2;
        border: tall $primary;
        background: $primary 8%;
    }

    ExportPane .stats-line {
        color: $text-muted;
    }
    """

    def compose(self) -> ComposeResult:
        yield Label("📤 Export Data to CSV", classes="export-title")
        yield Label(
            "Export all recorded screen time data to a CSV file.\n"
            "The CSV includes: start time, end time, duration, app, website, and category.",
            classes="export-description",
        )

        # Data summary
        min_date, max_date = self.db.get_available_dates()
        with Static(classes="stats-summary"):
            if min_date and max_date:
                yield Label(
                    f"📊 Data available from {min_date.strftime('%B %d, %Y')} "
                    f"to {max_date.strftime('%B %d, %Y')}",
                    classes="stats-line",
                )
            else:
                yield Label("📊 No data recorded yet.", classes="stats-line")

        # Export path
        default_path = str(
            Path.home()
            / f"screentime_export_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"
        )
        yield Label("Export file path:", classes="export-path-label")
        yield Input(
            value=default_path,
            id="export-path-input",
            classes="export-path-input",
        )

        yield Button(
            "Export All Data",
            id="export-btn",
            variant="primary",
            classes="export-btn",
        )

        yield Static(id="export-result", classes="export-result")

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id != "export-btn":
            return

        path_input = self.query_one("#export-path-input", Input)
        filepath = Path(path_input.value.strip())

        result_widget = self.query_one("#export-result", Static)
        result_widget.remove_children()

        try:
            # Ensure parent directory exists
            filepath.parent.mkdir(parents=True, exist_ok=True)

            self.db.export_csv(filepath)

            # Get file size
            size = filepath.stat().st_size
            if size > 1024 * 1024:
                size_str = f"{size / (1024 * 1024):.1f} MB"
            elif size > 1024:
                size_str = f"{size / 1024:.1f} KB"
            else:
                size_str = f"{size} bytes"

            result_widget.mount(
                Label(f"✅ Export successful!", classes="export-success")
            )
            result_widget.mount(
                Label(f"File: {filepath}\nSize: {size_str}", classes="export-info")
            )

        except Exception as e:
            result_widget.mount(
                Label(f"❌ Export failed: {e}", classes="export-error")
            )
