"""History panes — Daily, Weekly, and Monthly views."""

from datetime import date, timedelta

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.reactive import reactive
from textual.widget import Widget
from textual.widgets import Button, Label, Static

from screentime.database import Database
from screentime.tui.utils import format_duration, make_bar
from screentime.tui.dashboard import UsageRow, ChildUsageRow


class StatsView(Widget):
    """Reusable stats view showing total time and usage breakdown."""

    DEFAULT_CSS = """
    StatsView {
        height: 1fr;
        width: 100%;
    }

    StatsView .header-box {
        height: 5;
        width: 100%;
        background: $primary 12%;
        border: tall $primary;
        padding: 1 2;
        margin-bottom: 1;
    }

    StatsView .header-title {
        text-align: center;
        color: $text-muted;
    }

    StatsView .header-value {
        text-align: center;
        text-style: bold;
        color: $text;
    }

    StatsView .daily-totals {
        height: auto;
        margin-bottom: 1;
        padding: 0 1;
    }

    StatsView .daily-total-row {
        height: 1;
        layout: horizontal;
        padding: 0 1;
    }

    StatsView .daily-total-date {
        width: 20;
        color: $text-muted;
    }

    StatsView .daily-total-time {
        width: 12;
        color: #a78bfa;
    }

    StatsView .daily-total-bar {
        width: 1fr;
        color: #7c3aed;
    }

    StatsView .section-label {
        color: $accent;
        text-style: bold;
        margin: 1 0 0 0;
    }

    StatsView .usage-list {
        height: 1fr;
    }

    StatsView .empty-msg {
        text-align: center;
        color: $text-muted;
        margin: 4 0;
    }

    StatsView .info-row {
        height: 2;
        layout: horizontal;
        padding: 0 1;
        color: $text-muted;
    }

    StatsView .info-row .rank { width: 4; }
    StatsView .info-row .name { width: 28; text-style: italic; }
    StatsView .info-row .category-tag { width: 16; text-style: italic; }
    StatsView .info-row .time { width: 10; text-align: right; text-style: italic; }
    StatsView .info-row .bar { width: 1fr; padding: 0 1; text-style: italic; }
    """

    def __init__(
        self,
        db: Database,
        title: str,
        start_date: date,
        end_date: date,
        show_daily_totals: bool = False,
    ):
        super().__init__()
        self.db = db
        self.view_title = title
        self.start_date = start_date
        self.end_date = end_date
        self.show_daily_totals = show_daily_totals

    def compose(self) -> ComposeResult:
        stats = self.db.get_range_stats(self.start_date, self.end_date)
        total = sum(u.total_seconds for u in stats)

        with Static(classes="header-box"):
            yield Label(f"📅 {self.view_title}", classes="header-title")
            yield Label(
                f"Total Screen Time: {format_duration(total)}",
                classes="header-value",
            )

        # Show per-day totals for weekly/monthly views
        if self.show_daily_totals and self.start_date != self.end_date:
            yield Label("Daily Breakdown", classes="section-label")
            daily = self.db.get_daily_totals_in_range(self.start_date, self.end_date)
            max_daily = max(daily.values()) if daily else 1

            with VerticalScroll(classes="daily-totals"):
                for d, secs in sorted(daily.items()):
                    if secs < 1:
                        continue
                    frac = secs / max_daily if max_daily > 0 else 0
                    with Horizontal(classes="daily-total-row"):
                        yield Label(
                            d.strftime("%a, %b %d"),
                            classes="daily-total-date",
                        )
                        yield Label(
                            format_duration(secs),
                            classes="daily-total-time",
                        )
                        yield Label(
                            make_bar(frac, width=25),
                            classes="daily-total-bar",
                        )

        yield Label("App Usage", classes="section-label")

        with Horizontal(classes="info-row"):
            yield Label("#", classes="rank")
            yield Label("Application", classes="name")
            yield Label("Category", classes="category-tag")
            yield Label("Time", classes="time")
            yield Label("", classes="bar")

        if not stats:
            yield Label(
                "No activity recorded for this period.",
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


# ── Daily Pane ────────────────────────────────────────────────────────

class DailyPane(Widget):
    """Daily history view with date navigation."""

    DEFAULT_CSS = """
    DailyPane {
        height: 1fr;
        width: 100%;
    }

    DailyPane .nav-bar {
        height: 3;
        layout: horizontal;
        align: center middle;
        margin-bottom: 0;
    }

    DailyPane .nav-bar Button {
        min-width: 10;
        margin: 0 1;
    }

    DailyPane .nav-bar .date-display {
        width: 30;
        text-align: center;
        text-style: bold;
        padding: 1 0;
    }

    DailyPane .stats-area {
        height: 1fr;
    }
    """

    current_date: reactive[date] = reactive(date.today)

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        with Horizontal(classes="nav-bar"):
            yield Button("◀ Prev", id="daily-prev", variant="default")
            yield Label(
                self.current_date.strftime("%A, %B %d, %Y"),
                id="daily-date-label",
                classes="date-display",
            )
            yield Button("Next ▶", id="daily-next", variant="default")
            yield Button("Today", id="daily-today", variant="primary")

        with VerticalScroll(classes="stats-area", id="daily-stats"):
            yield StatsView(
                db=self.db,
                title=self.current_date.strftime("%A, %B %d, %Y"),
                start_date=self.current_date,
                end_date=self.current_date,
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "daily-prev":
            self.current_date = self.current_date - timedelta(days=1)
        elif event.button.id == "daily-next":
            self.current_date = self.current_date + timedelta(days=1)
        elif event.button.id == "daily-today":
            self.current_date = date.today()
        self._rebuild_stats()

    def _rebuild_stats(self):
        label = self.query_one("#daily-date-label", Label)
        label.update(self.current_date.strftime("%A, %B %d, %Y"))

        stats_area = self.query_one("#daily-stats", VerticalScroll)
        stats_area.remove_children()
        stats_area.mount(StatsView(
            db=self.db,
            title=self.current_date.strftime("%A, %B %d, %Y"),
            start_date=self.current_date,
            end_date=self.current_date,
        ))

    def refresh_data(self):
        self._rebuild_stats()


# ── Weekly Pane ───────────────────────────────────────────────────────

def _week_start(d: date) -> date:
    """Get Monday of the week containing date d."""
    return d - timedelta(days=d.weekday())


def _week_end(d: date) -> date:
    """Get Sunday of the week containing date d."""
    return _week_start(d) + timedelta(days=6)


class WeeklyPane(Widget):
    """Weekly history view with week navigation."""

    DEFAULT_CSS = """
    WeeklyPane {
        height: 1fr;
        width: 100%;
    }

    WeeklyPane .nav-bar {
        height: 3;
        layout: horizontal;
        align: center middle;
        margin-bottom: 0;
    }

    WeeklyPane .nav-bar Button {
        min-width: 10;
        margin: 0 1;
    }

    WeeklyPane .nav-bar .date-display {
        width: 40;
        text-align: center;
        text-style: bold;
        padding: 1 0;
    }

    WeeklyPane .stats-area {
        height: 1fr;
    }
    """

    current_week_start: reactive[date] = reactive(lambda: _week_start(date.today()))

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def _week_label(self) -> str:
        ws = self.current_week_start
        we = ws + timedelta(days=6)
        return f"{ws.strftime('%b %d')} — {we.strftime('%b %d, %Y')}"

    def compose(self) -> ComposeResult:
        with Horizontal(classes="nav-bar"):
            yield Button("◀ Prev", id="weekly-prev", variant="default")
            yield Label(
                self._week_label(),
                id="weekly-date-label",
                classes="date-display",
            )
            yield Button("Next ▶", id="weekly-next", variant="default")
            yield Button("This Week", id="weekly-current", variant="primary")

        ws = self.current_week_start
        we = ws + timedelta(days=6)
        with VerticalScroll(classes="stats-area", id="weekly-stats"):
            yield StatsView(
                db=self.db,
                title=self._week_label(),
                start_date=ws,
                end_date=we,
                show_daily_totals=True,
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "weekly-prev":
            self.current_week_start = self.current_week_start - timedelta(weeks=1)
        elif event.button.id == "weekly-next":
            self.current_week_start = self.current_week_start + timedelta(weeks=1)
        elif event.button.id == "weekly-current":
            self.current_week_start = _week_start(date.today())
        self._rebuild_stats()

    def _rebuild_stats(self):
        label = self.query_one("#weekly-date-label", Label)
        label.update(self._week_label())

        ws = self.current_week_start
        we = ws + timedelta(days=6)

        stats_area = self.query_one("#weekly-stats", VerticalScroll)
        stats_area.remove_children()
        stats_area.mount(StatsView(
            db=self.db,
            title=self._week_label(),
            start_date=ws,
            end_date=we,
            show_daily_totals=True,
        ))

    def refresh_data(self):
        self._rebuild_stats()


# ── Monthly Pane ──────────────────────────────────────────────────────

def _month_start(d: date) -> date:
    """First day of the month containing d."""
    return d.replace(day=1)


def _month_end(d: date) -> date:
    """Last day of the month containing d."""
    if d.month == 12:
        return d.replace(year=d.year + 1, month=1, day=1) - timedelta(days=1)
    return d.replace(month=d.month + 1, day=1) - timedelta(days=1)


class MonthlyPane(Widget):
    """Monthly history view with month navigation."""

    DEFAULT_CSS = """
    MonthlyPane {
        height: 1fr;
        width: 100%;
    }

    MonthlyPane .nav-bar {
        height: 3;
        layout: horizontal;
        align: center middle;
        margin-bottom: 0;
    }

    MonthlyPane .nav-bar Button {
        min-width: 10;
        margin: 0 1;
    }

    MonthlyPane .nav-bar .date-display {
        width: 30;
        text-align: center;
        text-style: bold;
        padding: 1 0;
    }

    MonthlyPane .stats-area {
        height: 1fr;
    }
    """

    current_month_start: reactive[date] = reactive(lambda: _month_start(date.today()))

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def _month_label(self) -> str:
        return self.current_month_start.strftime("%B %Y")

    def compose(self) -> ComposeResult:
        with Horizontal(classes="nav-bar"):
            yield Button("◀ Prev", id="monthly-prev", variant="default")
            yield Label(
                self._month_label(),
                id="monthly-date-label",
                classes="date-display",
            )
            yield Button("Next ▶", id="monthly-next", variant="default")
            yield Button("This Month", id="monthly-current", variant="primary")

        ms = self.current_month_start
        me = _month_end(ms)
        with VerticalScroll(classes="stats-area", id="monthly-stats"):
            yield StatsView(
                db=self.db,
                title=self._month_label(),
                start_date=ms,
                end_date=me,
                show_daily_totals=True,
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "monthly-prev":
            ms = self.current_month_start
            # Go to first day of previous month
            prev = ms - timedelta(days=1)
            self.current_month_start = _month_start(prev)
        elif event.button.id == "monthly-next":
            ms = self.current_month_start
            # Go to first day of next month
            nxt = _month_end(ms) + timedelta(days=1)
            self.current_month_start = _month_start(nxt)
        elif event.button.id == "monthly-current":
            self.current_month_start = _month_start(date.today())
        self._rebuild_stats()

    def _rebuild_stats(self):
        label = self.query_one("#monthly-date-label", Label)
        label.update(self._month_label())

        ms = self.current_month_start
        me = _month_end(ms)

        stats_area = self.query_one("#monthly-stats", VerticalScroll)
        stats_area.remove_children()
        stats_area.mount(StatsView(
            db=self.db,
            title=self._month_label(),
            start_date=ms,
            end_date=me,
            show_daily_totals=True,
        ))

    def refresh_data(self):
        self._rebuild_stats()
