"""Title Rules pane — manage app title splitting rules."""

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import (
    Button,
    Input,
    Label,
    ListItem,
    ListView,
    Static,
)

from screentime.database import Database


class RuleItem(ListItem):
    """A single title rule item."""

    def __init__(self, app_class: str, target_title: str):
        super().__init__()
        self.app_class = app_class
        self.target_title = target_title

    def compose(self) -> ComposeResult:
        yield Label(f"  {self.app_class}  →  {self.target_title}")


class RulesPane(Vertical):
    """Pane for configuring title split rules."""

    CSS = """
    RulesPane {
        padding: 1 2;
    }

    .rules-description {
        color: $text-muted;
        margin-bottom: 1;
    }

    .rules-form-label {
        margin-bottom: 0;
        color: $text;
    }

    #app-input, #title-input {
        margin-bottom: 1;
    }

    #btn-add-rule {
        margin-bottom: 1;
        width: 100%;
    }

    .rules-section-title {
        color: $accent;
        text-style: bold;
        margin: 1 0 0 0;
    }
    """

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        yield Label("🛠️ Title Splitting Rules", classes="section-title")
        yield Label(
            "Split specific window titles into their own apps.\n"
            "Example: Type 'kitty' as App Class and 'nvim' as Target Title.",
            classes="rules-description"
        )

        yield Label("App Class (e.g. kitty):", classes="rules-form-label")
        yield Input(placeholder="kitty", id="app-input")
        yield Label("Target Title (e.g. nvim):", classes="rules-form-label")
        yield Input(placeholder="nvim", id="title-input")
        yield Button("Add Rule", id="btn-add-rule", variant="success")

        yield Label("Current Rules:", classes="rules-section-title")
        yield ListView(id="rules-list")

    def on_mount(self):
        self._refresh_data()

    def _refresh_data(self):
        """Reload rules from database."""
        rules = self.db.get_title_rules()
        list_view = self.query_one("#rules-list", ListView)
        list_view.clear()

        for app_class, targets in sorted(rules.items()):
            for target in sorted(targets):
                list_view.append(RuleItem(app_class, target))

    def on_button_pressed(self, event: Button.Pressed):
        """Handle button presses."""
        if event.button.id == "btn-add-rule":
            app_input = self.query_one("#app-input", Input)
            title_input = self.query_one("#title-input", Input)

            app_class = app_input.value.strip()
            target_title = title_input.value.strip()

            if app_class and target_title:
                self.db.add_title_rule(app_class, target_title)
                title_input.value = ""
                app_input.value = ""
                self._refresh_data()

    def on_list_view_selected(self, event: ListView.Selected):
        """Handle clicking a rule to remove it."""
        item = event.item
        if isinstance(item, RuleItem):
            self.db.remove_title_rule(item.app_class, item.target_title)
            self._refresh_data()
