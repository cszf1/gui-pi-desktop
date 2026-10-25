---
name: gui-basics
description: How to observe and act in desktop apps through the gate's tools - reads, element tokens vs x/y, batches, diffs, screenshots, coordinates, menus, scrolling, new windows. Load at the start of any GUI task.
---
# GUI basics
- `list_apps` / `list_windows` find the pid and window_id; `launch_app` starts an app; `bring_to_front` raises a window.
- `get_window_state pid window_id` returns the accessibility tree; rows look like `[N] role "label"`. Use `element_token` `snapshot:N` from the NEWEST read. Pass `include_screenshot:true` only when you need pixels (canvas, images, unlabeled icons).
- A result's tree is often a diff ("Only rows that changed since snapshot X"): unlisted rows of snapshot X are unchanged and keep their [N]. Rows the client lists as gone or renumbered: their old tokens no longer work.
- Prefer element tokens over x/y. Aim by x/y only on a screenshot you just received. Coordinates are window-relative, in the space of that screenshot as the gate gives it; never scale them yourself (display scaling is the gate's and driver's job).
- Screenshots: each has an id (img-N). Only the newest one per window stays in view; `pin_evidence {id, label}` keeps up to 3 you must compare with or copy from; `recall_image {id}` shows any earlier one again.
- When you already see the targets of several steps (click a field, type, press Enter, save), send them in ONE `run_actions` call (`steps: [{tool, args}]`). It stops at the first step that fails or that the gate holds.
- A new dialog or menu is a new window: the gate tells you its window_id; act on its rows.
- Text: click the field (or use its token), then `type_text`. `set_value` replaces a field's value directly when the role allows it.
- Keys: `press_key key=Return`, `hotkey keys=["ctrl","s"]`. Keyboard shortcuts and menus are often more reliable than small icons.
- Lists and long pages: `scroll` on the element or window; tokens change after scrolling, use the attached tree.
