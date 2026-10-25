---
name: platform-windows
description: Windows desktop specifics - UI Automation trees, UAC prompts, elevated windows, DPI. Load on Windows.
---
# Windows desktop
- Trees come from UI Automation. A window that exposes almost nothing is drawn by the app itself: aim on its screenshot.
- A UAC prompt is on the secure desktop: you cannot see or click it. Ask for a takeover.
- Windows of elevated (administrator) apps cannot receive input from a normal-level agent: ask for a takeover.
- run_command does not run bash on Windows: write commands for the shell it reports (inspect kind=env).
