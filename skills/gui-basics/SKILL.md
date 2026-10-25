---
name: gui-basics
description: How to observe and act in desktop apps through the gate's tools - reads, tokens vs x/y, batches, menus, scrolling, new windows. Load at the start of any GUI task.
---
# GUI basics
- `list_apps` / `list_windows` find the pid and window_id; `launch_app` starts an app; `bring_to_front` raises a window.
- `get_window_state pid window_id` returns the accessibility tree; rows look like `[N] role "label"`. Use `element_token` `snapshot:N` from the NEWEST read. Pass `include_screenshot:true` only when you need pixels (canvas, images, unlabeled icons).
- After any action the result already contains the window's fresh tree (often as a diff). Use it; do not read again.
- A new dialog or menu is a new window: the gate tells you its window_id; act on its rows.
- `run_actions` takes `steps: [{tool, args}]`. It stops at the first step that fails or that the gate holds; steps after it did not run.
- Text: click the field (or use its token), then `type_text`. `set_value` replaces a field's value directly when the role allows it.
- Keys: `press_key key=Return`, `hotkey keys=["ctrl","s"]`.
- Lists and long pages: `scroll` on the element or window; tokens change after scrolling, use the attached tree.
- If a target is missing from the tree (canvas, grid), take a screenshot and aim by x/y in window coordinates.
