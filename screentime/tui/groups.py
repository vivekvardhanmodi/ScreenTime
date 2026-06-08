"""Groups pane — manage app/website grouping (aliases).

Groups combine multiple apps/websites into a single virtual entry
for display purposes (e.g., foot + kitty = Terminal).
Individual tracking is preserved in the database.
"""

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import ModalScreen
from textual.widget import Widget
from textual.widgets import (
    Button,
    Input,
    Label,
    OptionList,
    Static,
)
from textual.widgets.option_list import Option

from screentime.database import Database


# ── Modal: Create/Select Group ────────────────────────────────────────

class AddToGroupModal(ModalScreen[str | None]):
    """Modal to add an app to a group — create new or select existing."""

    DEFAULT_CSS = """
    AddToGroupModal {
        align: center middle;
        background: $surface 80%;
    }

    AddToGroupModal .dialog {
        width: 55;
        height: auto;
        max-height: 70%;
        background: $surface;
        border: thick $primary;
        padding: 2 3;
    }

    AddToGroupModal .dialog-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    AddToGroupModal .dialog-subtitle {
        color: $text-muted;
        margin-bottom: 1;
    }

    AddToGroupModal .dialog-section {
        color: #f59e0b;
        text-style: bold;
        margin: 1 0 0 0;
    }

    AddToGroupModal .group-options {
        height: auto;
        max-height: 10;
        border: tall $primary;
        margin-bottom: 1;
    }

    AddToGroupModal .new-group-input {
        margin-bottom: 1;
    }

    AddToGroupModal .dialog-buttons {
        height: 3;
        layout: horizontal;
        align: right middle;
        margin-top: 1;
    }

    AddToGroupModal .dialog-buttons Button {
        margin: 0 1;
    }
    """

    def __init__(self, app_name: str, existing_groups: list[str]):
        super().__init__()
        self.app_name = app_name
        self.existing_groups = existing_groups

    def compose(self) -> ComposeResult:
        with Vertical(classes="dialog"):
            yield Label("Add to Group", classes="dialog-title")
            yield Label(f"App: {self.app_name}", classes="dialog-subtitle")

            if self.existing_groups:
                yield Label("Existing Groups", classes="dialog-section")
                opts = OptionList(classes="group-options", id="group-select")
                for g in self.existing_groups:
                    opts.add_option(Option(g, id=g))
                yield opts

            yield Label("Or Create New Group", classes="dialog-section")
            yield Input(
                placeholder="New group name...",
                id="new-group-input",
                classes="new-group-input",
            )

            with Horizontal(classes="dialog-buttons"):
                yield Button("Cancel", id="cancel-btn", variant="default")
                yield Button("Remove from Group", id="remove-btn", variant="warning")
                yield Button("Apply", id="apply-btn", variant="primary")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "apply-btn":
            # Prefer new group name if filled
            new_input = self.query_one("#new-group-input", Input)
            new_name = new_input.value.strip()
            if new_name:
                self.dismiss(new_name)
                return

            # Otherwise use selected existing group
            if self.existing_groups:
                try:
                    opt_list = self.query_one("#group-select", OptionList)
                    idx = opt_list.highlighted
                    if idx is not None and 0 <= idx < len(self.existing_groups):
                        self.dismiss(self.existing_groups[idx])
                        return
                except Exception:
                    pass

            self.dismiss(None)
        elif event.button.id == "remove-btn":
            self.dismiss("__REMOVE__")
        else:
            self.dismiss(None)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        name = event.value.strip()
        if name:
            self.dismiss(name)


# ── Groups Pane ───────────────────────────────────────────────────────

class GroupsPane(Widget):
    """Group management pane."""

    DEFAULT_CSS = """
    GroupsPane {
        height: 1fr;
        width: 100%;
        layout: horizontal;
    }

    GroupsPane .left-panel {
        width: 40;
        height: 1fr;
        padding: 0 1 0 0;
    }

    GroupsPane .right-panel {
        width: 1fr;
        height: 1fr;
        padding: 0 0 0 1;
    }

    GroupsPane .panel-title {
        color: $accent;
        text-style: bold;
        margin-bottom: 1;
    }

    GroupsPane .group-list {
        height: 1fr;
        border: tall $primary;
    }

    GroupsPane .group-buttons {
        height: 3;
        layout: horizontal;
        margin-top: 1;
    }

    GroupsPane .group-buttons Button {
        margin: 0 1;
    }

    GroupsPane .app-list {
        height: 1fr;
        border: tall $primary;
    }

    GroupsPane .group-detail {
        height: auto;
        padding: 1;
        margin-bottom: 1;
        background: $primary 8%;
        border: tall $primary;
    }

    GroupsPane .detail-title {
        color: #f59e0b;
        text-style: bold;
    }

    GroupsPane .detail-member {
        color: $text-muted;
        padding: 0 0 0 2;
    }
    """

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        # Left panel: group list
        with Vertical(classes="left-panel"):
            yield Label("📦 Groups", classes="panel-title")
            self._build_group_list()

            with Horizontal(classes="group-buttons"):
                yield Button("Delete Group", id="group-del-btn", variant="error")

        # Right panel: app list
        with Vertical(classes="right-panel"):
            yield Label("🖥  Apps & Websites (click to group)", classes="panel-title")
            yield self._build_group_detail()
            self._build_app_list()

    def _build_group_list(self) -> OptionList:
        """Build the groups option list."""
        groups = self.db.get_groups()
        group_list = OptionList(id="groups-option-list", classes="group-list")
        for name, members in groups.items():
            member_names = ", ".join(m[0] for m in members)
            label = f"📦 {name} ({len(members)})"
            group_list.add_option(Option(label, id=name))
        return group_list

    def _build_app_list(self) -> OptionList:
        """Build the app/website option list with current group assignments."""
        all_apps = self.db.get_all_app_identifiers()
        group_map = self.db._get_group_map()

        app_list = OptionList(id="apps-group-list", classes="app-list")
        for name, id_type in all_apps:
            icon = "🌐" if id_type == "website" else "🖥 "
            grp = group_map.get(name, "—")
            label = f"{icon} {name:<30} [{grp}]"
            app_list.add_option(Option(label, id=f"{id_type}:{name}"))
        return app_list

    def _build_group_detail(self) -> Static:
        """Build group detail panel showing currently selected group members."""
        return Static(id="group-detail-box", classes="group-detail")

    def on_mount(self) -> None:
        """Build lists after mount."""
        left = self.query(".left-panel")
        right = self.query(".right-panel")
        if left:
            try:
                self.query_one("#groups-option-list", OptionList)
            except Exception:
                left[0].mount(self._build_group_list(), before=0)
        if right:
            try:
                self.query_one("#apps-group-list", OptionList)
            except Exception:
                right[0].mount(self._build_app_list())

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "group-del-btn":
            self._delete_selected_group()

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        """Handle selection — app list opens group modal, group list shows details."""
        if event.option_list.id == "apps-group-list":
            option_id = event.option.id
            if not option_id or ":" not in option_id:
                return

            id_type, name = option_id.split(":", 1)
            existing_groups = self.db.get_group_names()

            self._pending_app = (name, id_type)
            self.app.push_screen(
                AddToGroupModal(name, existing_groups),
                callback=self._on_group_selected,
            )

        elif event.option_list.id == "groups-option-list":
            # Show group detail
            group_name = event.option.id
            if group_name:
                self._show_group_detail(group_name)

    def _show_group_detail(self, group_name: str):
        """Update the detail panel to show members of a group."""
        groups = self.db.get_groups()
        members = groups.get(group_name, [])

        detail = self.query_one("#group-detail-box", Static)
        detail.remove_children()
        detail.mount(Label(f"📦 {group_name}", classes="detail-title"))
        for name, id_type in members:
            icon = "🌐" if id_type == "website" else "🖥 "
            detail.mount(Label(f"  {icon} {name}", classes="detail-member"))

    def _on_group_selected(self, group_name: str | None) -> None:
        if not hasattr(self, "_pending_app"):
            return

        name, id_type = self._pending_app
        del self._pending_app

        if group_name is None:
            return
        elif group_name == "__REMOVE__":
            self.db.remove_from_group(name)
        else:
            self.db.add_to_group(group_name, name, id_type)

        self.refresh_data()

    def _delete_selected_group(self):
        try:
            group_list = self.query_one("#groups-option-list", OptionList)
        except Exception:
            return

        idx = group_list.highlighted
        group_names = self.db.get_group_names()

        if idx is not None and 0 <= idx < len(group_names):
            self.db.delete_group(group_names[idx])
            self.refresh_data()

    def refresh_data(self):
        """Rebuild the pane."""
        self.remove_children()

        # Rebuild left panel
        left = Vertical(classes="left-panel")
        self.mount(left)
        left.mount(Label("📦 Groups", classes="panel-title"))

        groups = self.db.get_groups()
        group_list = OptionList(id="groups-option-list", classes="group-list")
        for name, members in groups.items():
            label = f"📦 {name} ({len(members)})"
            group_list.add_option(Option(label, id=name))
        left.mount(group_list)

        buttons = Horizontal(classes="group-buttons")
        left.mount(buttons)
        buttons.mount(Button("Delete Group", id="group-del-btn", variant="error"))

        # Rebuild right panel
        right = Vertical(classes="right-panel")
        self.mount(right)
        right.mount(
            Label("🖥  Apps & Websites (click to group)", classes="panel-title")
        )
        right.mount(Static(id="group-detail-box", classes="group-detail"))

        all_apps = self.db.get_all_app_identifiers()
        group_map = self.db._get_group_map()

        app_list = OptionList(id="apps-group-list", classes="app-list")
        for name, id_type in all_apps:
            icon = "🌐" if id_type == "website" else "🖥 "
            grp = group_map.get(name, "—")
            label = f"{icon} {name:<30} [{grp}]"
            app_list.add_option(Option(label, id=f"{id_type}:{name}"))
        right.mount(app_list)
