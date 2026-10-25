---
name: files
description: File manager and file dialogs - paths, Save/Open dialogs, overwriting. Load for tasks that create, move or save files.
---
# Files
- In Open/Save dialogs type the full path into the name field (ctrl+l in GTK dialogs shows a path box).
- Overwrite prompts are risky steps: the gate decides; if the user asked to overwrite that file, say so in gate_task words.
- Check the result with `run_command ls -l <path>` or by reading the file manager.
