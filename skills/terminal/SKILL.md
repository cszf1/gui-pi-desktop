---
name: terminal
description: Running commands - run_command vs typing into a terminal window, and what the gate stops. Load for file, shell or system tasks.
---
# Terminal and commands
- `run_command` runs a shell command and returns its output; it passes the same risk rules as GUI steps (deletes outside your own scratch files, overwrites, network sends, system changes need the user's words, the reviewer, or a person).
- Typing into a terminal window works too; the gate shows the output after Return. A refused risky Enter clears the input line.
- Prefer non-interactive commands; never run a command that waits for input without providing it.
