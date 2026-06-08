"""Dashboard pane — Today's screen time stats."""

from datetime import date

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.reactive import reactive
from textual.widget import Widget
from textual.widgets import Label, Static

from screentime.database import AppUsage, Database
from screentime.tui.utils import format_duration, make_bar


class UsageRow(Widget):
    """A single row showing app/website/group usage."""

    DEFAULT_CSS = """
    UsageRow {
        height: 2;
        layout: horizontal;
        padding: 0 1;
    }

    UsageRow:hover {
        background: $primary 10%;
    }

    UsageRow .rank {
        width: 4;
        color: $text-muted;
    }

    UsageRow .name {
        width: 28;
        text-style: bold;
    }

    UsageRow .name-website {
        width: 28;
        color: #60a5fa;
        text-style: bold;
    }

    UsageRow .name-group {
        width: 28;
        color: #f59e0b;
        text-style: bold;
    }

    UsageRow .category-tag {
        width: 16;
        color: $text-muted;
    }

    UsageRow .time {
        width: 10;
        text-align: right;
        color: #a78bfa;
        text-style: bold;
    }

    UsageRow .bar {
        width: 1fr;
        padding: 0 1;
        color: #7c3aed;
    }
    """

    def __init__(
        self,
        rank: int,
        usage: AppUsage,
        max_seconds: float,
    ):
        super().__init__()
        self.rank = rank
        self.usage = usage
        self.max_seconds = max_seconds

    def compose(self) -> ComposeResult:
        u = self.usage
        yield Label(f"{self.rank:>2}.", classes="rank")

        if u.identifier_type == "group":
            name_class = "name-group"
            icon = "📦 "
        elif u.identifier_type == "website":
            name_class = "name-website"
            icon = "🌐 "
        else:
            name_class = "name"
            icon = "🖥  "
        yield Label(f"{icon}{u.name}", classes=name_class)

        cat_text = u.category or "—"
        yield Label(cat_text, classes="category-tag")

        yield Label(format_duration(u.total_seconds), classes="time")

        fraction = u.total_seconds / self.max_seconds if self.max_seconds > 0 else 0
        bar_text = make_bar(fraction, width=30)
        yield Label(bar_text, classes="bar")


class ChildUsageRow(Widget):
    """An indented sub-row showing an individual app within a group."""

    DEFAULT_CSS = """
    ChildUsageRow {
        height: 1;
        layout: horizontal;
        padding: 0 1 0 6;
        color: $text-muted;
    }

    ChildUsageRow .child-name {
        width: 26;
    }

    ChildUsageRow .child-time {
        width: 10;
        text-align: right;
        color: #818cf8;
    }

    ChildUsageRow .child-bar {
        width: 1fr;
        padding: 0 1;
        color: #6366f1;
    }
    """

    def __init__(self, usage: AppUsage, max_seconds: float):
        super().__init__()
        self.usage = usage
        self.max_seconds = max_seconds

    def compose(self) -> ComposeResult:
        u = self.usage
        icon = "🌐" if u.identifier_type == "website" else "🖥 "
        yield Label(f"  └ {icon} {u.name}", classes="child-name")
        yield Label(format_duration(u.total_seconds), classes="child-time")
        fraction = u.total_seconds / self.max_seconds if self.max_seconds > 0 else 0
        yield Label(make_bar(fraction, width=20), classes="child-bar")


class DashboardPane(Widget):
    """Today's usage dashboard with live stats."""

    DEFAULT_CSS = """
    DashboardPane {
        height: 1fr;
        width: 100%;
    }

    DashboardPane .header-box {
        height: 5;
        width: 100%;
        background: $primary 12%;
        border: tall $primary;
        content-align: center middle;
        padding: 1 2;
        margin-bottom: 1;
    }

    DashboardPane .header-title {
        text-align: center;
        color: $text-muted;
    }

    DashboardPane .header-value {
        text-align: center;
        text-style: bold;
        color: $text;
    }

    DashboardPane .section-label {
        color: $accent;
        text-style: bold;
        margin: 1 0 0 0;
    }

    DashboardPane .usage-list {
        height: 1fr;
    }

    DashboardPane .empty-msg {
        text-align: center;
        color: $text-muted;
        margin: 4 0;
    }

    DashboardPane .info-row {
        height: 2;
        layout: horizontal;
        padding: 0 1;
        color: $text-muted;
    }

    DashboardPane .info-row .rank {
        width: 4;
    }

    DashboardPane .info-row .name {
        width: 28;
        text-style: italic;
    }

    DashboardPane .info-row .category-tag {
        width: 16;
        text-style: italic;
    }

    DashboardPane .info-row .time {
        width: 10;
        text-align: right;
        text-style: italic;
    }

    DashboardPane .info-row .bar {
        width: 1fr;
        padding: 0 1;
        text-style: italic;
    }
    """

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        today = date.today()
        total = self.db.get_total_time_for_date(today)
        stats = self.db.get_daily_stats(today)

        with Static(classes="header-box"):
            yield Label(f"📅 {today.strftime('%A, %B %d, %Y')}", classes="header-title")
            yield Label(f"Total Screen Time: {format_duration(total)}", classes="header-value")

        yield Label("App Usage", classes="section-label")

        # Column headers
        with Horizontal(classes="info-row"):
            yield Label("#", classes="rank")
            yield Label("Application", classes="name")
            yield Label("Category", classes="category-tag")
            yield Label("Time", classes="time")
            yield Label("", classes="bar")

        if not stats:
            yield Label(
                "No activity recorded yet today.\n"
                "Make sure the screentime-daemon is running.",
                classes="empty-msg",
            )
        else:
            max_secs = stats[0].total_seconds if stats else 1
            with VerticalScroll(classes="usage-list"):
                for i, usage in enumerate(stats, 1):
                    yield UsageRow(rank=i, usage=usage, max_seconds=max_secs)
                    if usage.children:
                        for child in usage.children:
                            yield ChildUsageRow(usage=child, max_seconds=max_secs)

    def refresh_data(self):
        """Re-render with fresh data."""
        self.remove_children()
        # Re-compose — Textual doesn't have a direct re-compose,
        # so we manually rebuild
        today = date.today()
        total = self.db.get_total_time_for_date(today)
        stats = self.db.get_daily_stats(today)

        header_box = Static(classes="header-box")
        self.mount(header_box)
        header_box.mount(
            Label(f"📅 {today.strftime('%A, %B %d, %Y')}", classes="header-title")
        )
        header_box.mount(
            Label(f"Total Screen Time: {format_duration(total)}", classes="header-value")
        )

        self.mount(Label("App Usage", classes="section-label"))

        header_row = Horizontal(classes="info-row")
        self.mount(header_row)
        header_row.mount(Label("#", classes="rank"))
        header_row.mount(Label("Application", classes="name"))
        header_row.mount(Label("Category", classes="category-tag"))
        header_row.mount(Label("Time", classes="time"))
        header_row.mount(Label("", classes="bar"))

        if not stats:
            self.mount(Label(
                "No activity recorded yet today.\n"
                "Make sure the screentime-daemon is running.",
                classes="empty-msg",
            ))
        else:
            max_secs = stats[0].total_seconds if stats else 1
            scroll = VerticalScroll(classes="usage-list")
            self.mount(scroll)
            for i, usage in enumerate(stats, 1):
                scroll.mount(UsageRow(rank=i, usage=usage, max_seconds=max_secs))
                if usage.children:
                    for child in usage.children:
                        scroll.mount(ChildUsageRow(usage=child, max_seconds=max_secs))
