## 2024-05-15 - [Form Labels and Icon Buttons]
**Learning:** Found an accessibility issue pattern across this app's components: forms were frequently using `<span>` with class `form-label` instead of proper `<label htmlFor="...">` associated with input `id`s, and delete actions in tables lacked `aria-label`s on their icon-only buttons.
**Action:** Always verify that `<label>`s use the `htmlFor` attribute targeting an element with an `id`, and ensure all icon-only action buttons (like delete) have a descriptive `aria-label` for screen readers.
