# Pi Connect (Windows app, tabs)

Tauri app that opens https://connect.raspberrypi.com in tabs.
- `+` new tab, `✕` (or middle click) close
- `⧉` detach a tab into its own window (needs 2+ tabs)
- `↩` (in a detached window) attach the tab back to the main window
- Only raspberrypi.com / raspberrypi.org can be opened (see ALLOWED_DOMAINS in src-tauri/src/main.rs)

## Run / build
    cd src-tauri
    cargo tauri dev
    cargo tauri build
