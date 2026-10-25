---
name: terminal
description: Running commands - run_command vs typing into a terminal window, long jobs, and what the gate holds back. Load for file, shell or system tasks.
---
# Terminal and commands
- `run_command` passes the same risk rules as GUI steps: deletes outside your own scratch files, overwrites, network sends and system changes need the user's words, the reviewer, or a person.
- Typing into a terminal window works too; the gate shows the output after Return. A refused risky Enter clears the input line.
- Prefer non-interactive commands; never run one that waits for input without providing it.
