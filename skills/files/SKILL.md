---
name: files
description: File manager and file dialogs - paths, Save/Open dialogs, overwriting. Load for tasks that create, move or save files.
---
# Files
- In Open/Save dialogs type the full path into the name field (ctrl+l in GTK dialogs shows a path box).
- Overwrite prompts are risky steps: the gate decides from the user's own request, its reviewer or the person. Nothing you say authorises it; if it is refused, save under another name or ask with request_human answer.
- Check the result with `run_command ls -l <path>` or by reading the file manager.
