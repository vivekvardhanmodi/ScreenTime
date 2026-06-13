"""Title Rules pane — manage app title splitting rules."""

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import (
    Button,
    Input,
    Label,
    ListItem,
    ListView,
    Select,
)

from screentime.database import Database


class RuleItem(ListItem):
    """A single title rule item."""

    def __init__(self, app_class: str, target_title: str):
        super().__init__()
        self.app_class = app_class
        self.target_title = target_title

    def compose(self) -> ComposeResult:
        yield Horizontal(
            Label(f"{self.app_class} → {self.target_title}", classes="rule-label"),
            Button("Remove", id="btn-remove-rule", variant="error"),
            classes="rule-row",
        )


class RulesPane(Vertical):
    """Pane for configuring title split rules."""

    CSS = """
    RulesPane {
        padding: 1 2;
    }

    .panel-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    .form-row {
        height: 3;
        margin-bottom: 1;
    }

    #app-select {
        width: 30%;
    }

    #title-input {
        width: 1fr;
        margin-left: 1;
    }

    #btn-add-rule {
        margin-left: 1;
    }

    .rule-row {
        height: 3;
        align: left middle;
    }

    .rule-label {
        width: 1fr;
        content-align: left middle;
    }

    #btn-remove-rule {
        margin-left: 1;
    }
    """

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        yield Label("🛠️ Title Splitting Rules", classes="panel-title")
        yield Label(
            "Split specific window titles into their own standalone apps.\n"
            "Example: Type 'kitty' and add 'nvim' to track Neovim separately.",
            classes="text-muted"
        )

        yield Horizontal(
            Input(placeholder="App Class (e.g. kitty)", id="app-input"),
            Input(placeholder="Target Title (e.g. nvim)", id="title-input"),
            Button("Add Rule", id="btn-add-rule", variant="success"),
            classes="form-row"
        )

        yield VerticalScroll(
            ListView(id="rules-list"),
            id="rules-container"
        )

    def on_mount(self):
        self._refresh_data()

    def _refresh_data(self):
        """Reload rules from database."""
        # Update rules list
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

        elif event.button.id == "btn-remove-rule":
            item = event.button.parent
            if isinstance(item, Horizontal):
                rule_item = item.parent
                if isinstance(rule_item, RuleItem):
                    self.db.remove_title_rule(rule_item.app_class, rule_item.target_title)
                    self._refresh_data()
