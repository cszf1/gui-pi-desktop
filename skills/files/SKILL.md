---
name: files
description: File managers and Open/Save dialogs - typing paths, overwrite prompts, checking the result. Load for tasks that create, move or save files.
---
# Files
- In GTK Open/Save dialogs `ctrl+l` shows a path box: type the full path into it.
- An overwrite prompt is a risky step: if the gate refuses it, save under another name or ask with request_human answer.
- Check the result with `run_command ls -l <path>` or by reading the file manager.
