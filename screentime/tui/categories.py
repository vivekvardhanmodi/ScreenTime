"""Categories pane — manage app/website category assignments."""

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


# ── Modal: Create Category ────────────────────────────────────────────

class CreateCategoryModal(ModalScreen[str | None]):
    """Modal dialog to create a new category."""

    DEFAULT_CSS = """
    CreateCategoryModal {
        align: center middle;
        background: $surface 80%;
    }

    CreateCategoryModal .dialog {
        width: 50;
        height: auto;
        background: $surface;
        border: thick $primary;
        padding: 2 3;
    }

    CreateCategoryModal .dialog-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    CreateCategoryModal .dialog-buttons {
        height: 3;
        layout: horizontal;
        align: right middle;
        margin-top: 1;
    }

    CreateCategoryModal .dialog-buttons Button {
        margin: 0 1;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(classes="dialog"):
            yield Label("Create New Category", classes="dialog-title")
            yield Input(placeholder="Category name...", id="category-name-input")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Cancel", id="cancel-btn", variant="default")
                yield Button("Create", id="create-btn", variant="primary")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "create-btn":
            inp = self.query_one("#category-name-input", Input)
            name = inp.value.strip()
            if name:
                self.dismiss(name)
            else:
                self.dismiss(None)
        else:
            self.dismiss(None)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        name = event.value.strip()
        self.dismiss(name if name else None)


# ── Modal: Assign App to Category ────────────────────────────────────

class AssignCategoryModal(ModalScreen[int | None]):
    """Modal to select a category for an app/website."""

    DEFAULT_CSS = """
    AssignCategoryModal {
        align: center middle;
        background: $surface 80%;
    }

    AssignCategoryModal .dialog {
        width: 50;
        height: auto;
        max-height: 70%;
        background: $surface;
        border: thick $primary;
        padding: 2 3;
    }

    AssignCategoryModal .dialog-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }

    AssignCategoryModal .dialog-subtitle {
        color: $text-muted;
        margin-bottom: 1;
    }

    AssignCategoryModal .category-options {
        height: auto;
        max-height: 15;
        border: tall $primary;
    }

    AssignCategoryModal .dialog-buttons {
        height: 3;
        layout: horizontal;
        align: right middle;
        margin-top: 1;
    }

    AssignCategoryModal .dialog-buttons Button {
        margin: 0 1;
    }
    """

    def __init__(self, app_name: str, categories: list):
        super().__init__()
        self.app_name = app_name
        self.categories = categories  # list of Category objects

    def compose(self) -> ComposeResult:
        with Vertical(classes="dialog"):
            yield Label("Assign to Category", classes="dialog-title")
            yield Label(f"App: {self.app_name}", classes="dialog-subtitle")

            options = OptionList(classes="category-options", id="category-select")
            for cat in self.categories:
                options.add_option(Option(cat.name, id=str(cat.id)))
            yield options

            with Horizontal(classes="dialog-buttons"):
                yield Button("Cancel", id="cancel-btn", variant="default")
                yield Button("Remove Category", id="remove-btn", variant="warning")
                yield Button("Assign", id="assign-btn", variant="primary")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "assign-btn":
            opt_list = self.query_one("#category-select", OptionList)
            idx = opt_list.highlighted
            if idx is not None and 0 <= idx < len(self.categories):
                self.dismiss(self.categories[idx].id)
            else:
                self.dismiss(None)
        elif event.button.id == "remove-btn":
            self.dismiss(-1)  # Special value: remove assignment
        else:
            self.dismiss(None)


# ── Categories Pane ───────────────────────────────────────────────────

class CategoriesPane(Widget):
    """Category management pane."""

    DEFAULT_CSS = """
    CategoriesPane {
        height: 1fr;
        width: 100%;
        layout: horizontal;
    }

    CategoriesPane .left-panel {
        width: 35;
        height: 1fr;
        padding: 0 1 0 0;
    }

    CategoriesPane .right-panel {
        width: 1fr;
        height: 1fr;
        padding: 0 0 0 1;
    }

    CategoriesPane .panel-title {
        color: $accent;
        text-style: bold;
        margin-bottom: 1;
    }

    CategoriesPane .cat-list {
        height: 1fr;
        border: tall $primary;
    }

    CategoriesPane .cat-buttons {
        height: 3;
        layout: horizontal;
        margin-top: 1;
    }

    CategoriesPane .cat-buttons Button {
        margin: 0 1;
    }

    CategoriesPane .app-list {
        height: 1fr;
        border: tall $primary;
    }

    CategoriesPane .app-item {
        height: 2;
        layout: horizontal;
        padding: 0 1;
    }

    CategoriesPane .app-item:hover {
        background: $primary 10%;
    }

    CategoriesPane .app-item-name {
        width: 1fr;
    }

    CategoriesPane .app-item-type {
        width: 10;
        color: $text-muted;
    }

    CategoriesPane .app-item-cat {
        width: 16;
        color: #a78bfa;
    }
    """

    def __init__(self, db: Database):
        super().__init__()
        self.db = db

    def compose(self) -> ComposeResult:
        # Left panel: category list
        with Vertical(classes="left-panel"):
            yield Label("📁 Categories", classes="panel-title")
            cat_list = OptionList(id="cat-option-list", classes="cat-list")
            categories = self.db.get_categories()
            for cat in categories:
                apps = self.db.get_apps_in_category(cat.id)
                cat_list.add_option(Option(f"{cat.name} ({len(apps)})", id=str(cat.id)))
            yield cat_list

            with Horizontal(classes="cat-buttons"):
                yield Button("+ New", id="cat-new-btn", variant="primary")
                yield Button("Delete", id="cat-del-btn", variant="error")

        # Right panel: app list with category assignments
        with Vertical(classes="right-panel"):
            yield Label("🖥  Apps & Websites (click to categorize)", classes="panel-title")
            self._build_app_list()

    def _build_app_list(self) -> OptionList:
        """Build the app/website option list."""
        all_apps = self.db.get_all_app_identifiers()
        category_map = self.db._get_category_map()

        app_list = OptionList(id="app-option-list", classes="app-list")
        for name, id_type in all_apps:
            icon = "🌐" if id_type == "website" else "🖥 "
            cat = category_map.get(name, "Uncategorized")
            label = f"{icon} {name:<30} [{cat}]"
            app_list.add_option(Option(label, id=f"{id_type}:{name}"))
        return app_list

    def on_mount(self) -> None:
        """Build app list after mount."""
        right = self.query(".right-panel")
        if right:
            try:
                existing = self.query_one("#app-option-list", OptionList)
            except Exception:
                app_list = self._build_app_list()
                right[0].mount(app_list)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "cat-new-btn":
            self.app.push_screen(
                CreateCategoryModal(),
                callback=self._on_category_created,
            )
        elif event.button.id == "cat-del-btn":
            self._delete_selected_category()

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        """Handle app selection — open assign category modal."""
        if event.option_list.id != "app-option-list":
            return

        option_id = event.option.id
        if not option_id or ":" not in option_id:
            return

        id_type, name = option_id.split(":", 1)
        categories = self.db.get_categories()

        if not categories:
            self.app.push_screen(
                CreateCategoryModal(),
                callback=self._on_category_created,
            )
            return

        self._pending_assign = (name, id_type)
        self.app.push_screen(
            AssignCategoryModal(name, categories),
            callback=self._on_category_assigned,
        )

    def _on_category_created(self, name: str | None) -> None:
        if name:
            try:
                self.db.create_category(name)
            except Exception:
                pass  # Duplicate name
        self.refresh_data()

    def _on_category_assigned(self, category_id: int | None) -> None:
        if not hasattr(self, "_pending_assign"):
            return

        name, id_type = self._pending_assign
        del self._pending_assign

        if category_id is None:
            return
        elif category_id == -1:
            # Remove assignment
            self.db.unassign_app_from_category(name)
        else:
            self.db.assign_app_to_category(name, id_type, category_id)

        self.refresh_data()

    def _delete_selected_category(self):
        try:
            cat_list = self.query_one("#cat-option-list", OptionList)
        except Exception:
            return

        idx = cat_list.highlighted
        categories = self.db.get_categories()

        if idx is not None and 0 <= idx < len(categories):
            self.db.delete_category(categories[idx].id)
            self.refresh_data()

    def refresh_data(self):
        """Rebuild the pane."""
        self.remove_children()

        # Rebuild left panel
        left = Vertical(classes="left-panel")
        self.mount(left)
        left.mount(Label("📁 Categories", classes="panel-title"))

        cat_list = OptionList(id="cat-option-list", classes="cat-list")
        categories = self.db.get_categories()
        for cat in categories:
            apps = self.db.get_apps_in_category(cat.id)
            cat_list.add_option(Option(f"{cat.name} ({len(apps)})", id=str(cat.id)))
        left.mount(cat_list)

        buttons = Horizontal(classes="cat-buttons")
        left.mount(buttons)
        buttons.mount(Button("+ New", id="cat-new-btn", variant="primary"))
        buttons.mount(Button("Delete", id="cat-del-btn", variant="error"))

        # Rebuild right panel
        right = Vertical(classes="right-panel")
        self.mount(right)
        right.mount(
            Label("🖥  Apps & Websites (click to categorize)", classes="panel-title")
        )

        all_apps = self.db.get_all_app_identifiers()
        category_map = self.db._get_category_map()

        app_list = OptionList(id="app-option-list", classes="app-list")
        for name, id_type in all_apps:
            icon = "🌐" if id_type == "website" else "🖥 "
            cat = category_map.get(name, "Uncategorized")
            label = f"{icon} {name:<30} [{cat}]"
            app_list.add_option(Option(label, id=f"{id_type}:{name}"))
        right.mount(app_list)
