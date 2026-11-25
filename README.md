# Pi Connect Desktop

A small Windows desktop app that opens [Raspberry Pi Connect](https://connect.raspberrypi.com) in its own window, with browser-style tabs that you can detach and re-attach. Handy when you keep several remote shells open at once and don't want them buried among your browser tabs.

Built with [Tauri v2](https://tauri.app) (Rust + WebView2).

> **Unofficial project.** It is not affiliated with, endorsed by, or supported by Raspberry Pi Ltd. It simply wraps the public Pi Connect website; you still sign in with your own Raspberry Pi ID.

## Features

- **Tabs**: `+` opens a new tab, `✕` closes it. Closing the last tab closes the window.
- **Detach / attach**: `⧉` moves a tab into its own window, `↩` moves it back to the main window. The tab is moved, not reloaded, so a running remote shell stays connected.
- **Ctrl+click**: Ctrl+click on a link inside Pi Connect opens it in a new tab. A normal click (or a link that wants a popup) stays in the current tab.
- **Device names as tab titles**: tabs are named after the device you are connected to (e.g. `neurogreen`) instead of a number. Names are learned from the device list page.
- **Stays signed in**: the session is kept between launches.
- **Locked to Raspberry Pi**: only `raspberrypi.com` and `raspberrypi.org` (and subdomains) can be loaded. Anything else is blocked.

## Requirements (to build)

- Windows 10 / 11
- [Rust](https://rustup.rs)
- Visual Studio Build Tools with the **Desktop development with C++** workload
- WebView2 runtime (already installed on most Windows 10/11 machines)

## Build and run

```powershell
cargo install tauri-cli --version "^2" --locked
cd src-tauri

cargo tauri dev      # run in development mode
cargo tauri build    # build the installer
```

The installer is created in `src-tauri\target\release\bundle\nsis\`.

The first build takes a while (several minutes); later ones are much faster.

## Custom icon

Put a square PNG (at least 512×512, ideally 1024×1024, transparent background) next to `Cargo.toml` and run:

```powershell
cargo tauri icon your-icon.png
```

This regenerates everything in `src-tauri\icons\`.

## Project layout

```
ui/index.html              Tab bar (plain HTML/JS)
src-tauri/src/main.rs      Window, tab and webview management
src-tauri/tauri.conf.json  App configuration
src-tauri/capabilities/    Permissions (tab bar only)
```

## How it works

Each window has one small webview for the tab bar (`ui/index.html`) and one webview per tab, all loading Pi Connect. The Rust side keeps track of which tab belongs to which window, lays the webviews out under the tab bar, and moves a tab's webview between windows when you detach or attach it.

A short script is injected into every Pi Connect page to handle Ctrl+click on links and to read device names from the device list. Pi Connect pages get no access to the app's commands.

To allow more domains (for example if the sign-in flow needs one), edit `ALLOWED_DOMAINS` in `src-tauri/src/main.rs`. Blocked addresses are printed in the terminal when running `cargo tauri dev`.

## Known limitations

- Multi-webview support in Tauri v2 is behind the `unstable` feature flag, so behaviour may change between Tauri versions.
- Only tested on Windows.
- Ctrl+click only works on real links. Buttons that navigate with JavaScript are not intercepted.
- A tab only gets its device name after the device list has been shown at least once in the app.
- No drag-and-drop of tabs, tab renaming, or keyboard shortcuts yet.

## Author

Kodjo AMOUZOUVI — [github.com/fkodjo](https://github.com/fkodjo)