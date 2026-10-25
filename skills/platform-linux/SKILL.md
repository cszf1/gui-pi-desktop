---
name: platform-linux
description: Linux desktop specifics - X11 vs Wayland, AT-SPI trees, GTK/Qt dialogs, desktop settings. Load on Linux when an app's tree looks empty, a window cannot be found, or for system settings.
---
# Linux desktop
- Supported: X11 sessions. Under Wayland input and screenshots of other apps are restricted; if a step keeps failing with nothing happening, say so instead of retrying.
- Trees come from AT-SPI. Apps without accessibility (some Electron/Java apps, games) show few rows: use a screenshot and x/y, or keyboard shortcuts.
- Qt apps may need `QT_ACCESSIBILITY=1`; if their tree is empty, report it rather than guessing.
- File dialogs differ by toolkit: GTK takes a typed path after `ctrl+l`, KDE/Qt dialogs have a path field at the top.
