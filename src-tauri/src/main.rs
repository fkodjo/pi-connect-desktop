#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{collections::HashMap, sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder}, window::WindowBuilder, AppHandle, Emitter, LogicalPosition,
    LogicalSize, Manager, Rect, WebviewUrl, Window, WindowEvent,
};

const HOME: &str = "https://connect.raspberrypi.com";
const BAR_H: f64 = 40.0;
// Only these domains (and their subdomains) can be opened in the tabs.
const ALLOWED_DOMAINS: &[&str] = &["raspberrypi.com", "raspberrypi.org"];

// Injected in every Pi Connect page: Ctrl+click / middle click on a link opens it as a new tab.
const HOOK_JS: &str = r#"
(function () {
  if (window.__piTabHook) return;
  window.__piTabHook = true;
  function findLink(e) {
    var el = e.target;
    while (el && el.tagName !== 'A') el = el.parentElement;
    return el && el.href ? el : null;
  }
  function handler(e) {
    if (!(e.ctrlKey || e.metaKey || e.button === 1)) return;
    var a = findLink(e);
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
    window.open(a.href, '_blank');
  }
  document.addEventListener('click', handler, true);
  document.addEventListener('auxclick', handler, true);
})();

// window.open() is denied by the app (the link becomes a tab instead). Many pages then fall back
// to navigating the current tab when window.open() returns null, which opened the link twice.
// Return a stand-in window object so the page believes the popup worked.
(function () {
  var orig = window.open;
  window.open = function () {
    var w = orig.apply(window, arguments);
    if (w) return w;
    var stub = {
      closed: false,
      close: function () {},
      focus: function () {},
      blur: function () {},
      postMessage: function () {},
      document: { write: function () {}, close: function () {} }
    };
    var go = function (u) { if (u) orig.call(window, String(u), '_blank'); };
    Object.defineProperty(stub, 'location', {
      get: function () {
        return { href: '', assign: go, replace: go };
      },
      set: go
    });
    return stub;
  };
})();

// Tab title = device name. Names are learned from the device list links and cached
// in localStorage (shared by all tabs), then applied on /devices/<uuid>... pages.
(function () {
  var KEY = '__piDeviceNames';
  var LINK = /\/devices\/([0-9a-f-]{36})\/?$/i;
  var PAGE = /\/devices\/([0-9a-f-]{36})/i;
  function load() {
    try { return JSON.parse(localStorage.getItem(KEY) || '{}'); } catch (e) { return {}; }
  }
  function scan() {
    var names = load();
    var changed = false;
    document.querySelectorAll('a[href*="/devices/"]').forEach(function (a) {
      var path;
      try { path = new URL(a.href, location.href).pathname; } catch (e) { return; }
      var r = LINK.exec(path);
      var t = (a.textContent || '').trim();
      if (r && t && t.length < 60 && t.indexOf('\n') < 0 && names[r[1]] !== t) {
        names[r[1]] = t;
        changed = true;
      }
    });
    if (changed) {
      try { localStorage.setItem(KEY, JSON.stringify(names)); } catch (e) {}
    }
    return names;
  }
  setInterval(function () {
    var names = scan();
    var r = PAGE.exec(location.pathname);
    var name = r ? names[r[1]] : null;
    if (name && document.title !== name) document.title = name;
  }, 1000);
})();
"#;

#[derive(Clone, Serialize)]
struct Tab {
    id: u32,
    title: String,
}

#[derive(Default)]
struct WinState {
    tabs: Vec<Tab>,
    active: Option<u32>,
}

#[derive(Default)]
struct AppState {
    wins: Mutex<HashMap<String, WinState>>,
    next: Mutex<u32>,
}

#[derive(Clone, Serialize)]
struct Snapshot {
    window: String,
    tabs: Vec<Tab>,
    active: Option<u32>,
    is_main: bool,
}

fn tab_label(id: u32) -> String {
    format!("tab-{id}")
}

fn next_id(app: &AppHandle) -> u32 {
    let st = app.state::<AppState>();
    let mut n = st.next.lock().unwrap();
    *n += 1;
    *n
}

fn allowed(url: &tauri::Url) -> bool {
    if url.scheme() == "about" {
        return true;
    }
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or("");
    ALLOWED_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn snapshot(app: &AppHandle, label: &str) -> Snapshot {
    let st = app.state::<AppState>();
    let wins = st.wins.lock().unwrap();
    let (tabs, active) = wins
        .get(label)
        .map(|w| (w.tabs.clone(), w.active))
        .unwrap_or_default();
    Snapshot {
        window: label.to_string(),
        tabs,
        active,
        is_main: label == "main",
    }
}

// Positions the tab bar and the tab webviews inside a window.
fn bounds(y: f64, w: f64, h: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(0.0, y).into(),
        size: LogicalSize::new(w.max(1.0), h.max(1.0)).into(),
    }
}

fn layout(app: &AppHandle, window: &Window) {
    layout_inner(app, window, false);
}

// `nudge` forces WebView2 to recompute its bounds (needed after a reparent).
fn layout_inner(app: &AppHandle, window: &Window, nudge: bool) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(size) = window.inner_size() else { return };
    let w = (size.width as f64 / scale).max(2.0);
    let h = (size.height as f64 / scale).max(2.0);
    let label = window.label().to_string();

    if let Some(ui) = app.get_webview(&format!("ui-{label}")) {
        if nudge {
            let _ = ui.set_bounds(bounds(0.0, w - 1.0, BAR_H));
        }
        let _ = ui.set_bounds(bounds(0.0, w, BAR_H));
    }

    // Copy the state first so the lock is not held while talking to webviews.
    let snap = snapshot(app, &label);
    for t in &snap.tabs {
        if let Some(wv) = app.get_webview(&tab_label(t.id)) {
            if nudge {
                let _ = wv.set_bounds(bounds(BAR_H, w - 1.0, h - BAR_H - 1.0));
            }
            let _ = wv.set_bounds(bounds(BAR_H, w, h - BAR_H));
            if snap.active == Some(t.id) {
                let _ = wv.show();
            } else {
                let _ = wv.hide();
            }
        }
    }
}

// Re-applies the layout a bit later, once the reparent has really settled.
fn relayout_later(app: &AppHandle, labels: Vec<String>) {
    let app = app.clone();
    std::thread::spawn(move || {
        for ms in [150u64, 500] {
            std::thread::sleep(Duration::from_millis(ms));
            for l in &labels {
                if let Some(w) = app.get_window(l) {
                    layout_inner(&app, &w, true);
                }
            }
        }
    });
}

fn refresh(app: &AppHandle, label: &str) {
    if let Some(win) = app.get_window(label) {
        layout(app, &win);
    }
    let _ = app.emit("tabs-changed", snapshot(app, label));
}

fn make_window(app: &AppHandle, label: &str, title: &str) -> tauri::Result<Window> {
    let window = WindowBuilder::new(app, label)
        .title(title)
        .inner_size(1280.0, 800.0)
        .build()?;
    window.add_child(
        WebviewBuilder::new(format!("ui-{label}"), WebviewUrl::App("index.html".into())),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(1280.0, BAR_H),
    )?;
    Ok(window)
}

fn add_tab(app: &AppHandle, label: &str, url: Option<tauri::Url>) -> Result<(), String> {
    let window = app.get_window(label).ok_or("window not found")?;
    let id = next_id(app);
    let url = match url {
        Some(u) => u,
        None => HOME.parse().map_err(|e| format!("{e}"))?,
    };
    let app_for_popups = app.clone();
    let app_for_title = app.clone();
    let builder = WebviewBuilder::new(tab_label(id), WebviewUrl::External(url))
        .initialization_script(HOOK_JS)
        .on_document_title_changed(move |_wv, title| {
            set_tab_title(&app_for_title, id, title);
        })
        .on_navigation(|u| {
            let ok = allowed(u);
            eprintln!("{} {u}", if ok { "Navigate:" } else { "Navigation blocked:" });
            ok
        })
        .on_new_window(move |u, _features| {
            eprintln!("New window request: {u}");
            if u.scheme() == "about" {
                // blank popup: the real address is assigned afterwards
            } else if allowed(&u) {
                open_link_in_tab(&app_for_popups, id, u);
            } else {
                eprintln!("Popup blocked: {u}");
            }
            NewWindowResponse::Deny
        });
    window
        .add_child(
            builder,
            LogicalPosition::new(0.0, BAR_H),
            LogicalSize::new(800.0, 600.0),
        )
        .map_err(|e| e.to_string())?;
    {
        let st = app.state::<AppState>();
        let mut wins = st.wins.lock().unwrap();
        let ws = wins.entry(label.to_string()).or_default();
        ws.tabs.push(Tab {
            id,
            title: format!("Pi {id}"),
        });
        ws.active = Some(id);
    }
    refresh(app, label);
    Ok(())
}

// Updates a tab's title (from the page's document title) and refreshes the tab bar.
fn set_tab_title(app: &AppHandle, id: u32, title: String) {
    let title = title.trim().to_string();
    if title.is_empty() {
        return;
    }
    let label = {
        let st = app.state::<AppState>();
        let mut wins = st.wins.lock().unwrap();
        let mut found = None;
        for (l, ws) in wins.iter_mut() {
            if let Some(t) = ws.tabs.iter_mut().find(|t| t.id == id) {
                if t.title != title {
                    t.title = title.clone();
                    found = Some(l.clone());
                }
                break;
            }
        }
        found
    };
    if let Some(l) = label {
        let _ = app.emit("tabs-changed", snapshot(app, &l));
    }
}

// Opens `url` as a new tab in the window that currently holds tab `from_tab`.
// Done on a separate thread: creating webviews from inside a webview callback can deadlock.
fn open_link_in_tab(app: &AppHandle, from_tab: u32, url: tauri::Url) {
    let app = app.clone();
    std::thread::spawn(move || {
        let label = {
            let st = app.state::<AppState>();
            let wins = st.wins.lock().unwrap();
            wins.iter()
                .find(|(_, ws)| ws.tabs.iter().any(|t| t.id == from_tab))
                .map(|(l, _)| l.clone())
        };
        if let Some(label) = label {
            let _ = add_tab(&app, &label, Some(url));
        }
    });
}

// Moves a tab's bookkeeping from one window to another. Returns true if the source is now empty.
fn move_tab_state(app: &AppHandle, from: &str, to: &str, id: u32) -> bool {
    let st = app.state::<AppState>();
    let mut wins = st.wins.lock().unwrap();
    let mut moved = None;
    let mut empty = false;
    if let Some(ws) = wins.get_mut(from) {
        if let Some(pos) = ws.tabs.iter().position(|t| t.id == id) {
            moved = Some(ws.tabs.remove(pos));
        }
        if ws.active == Some(id) {
            ws.active = ws.tabs.last().map(|t| t.id);
        }
        empty = ws.tabs.is_empty();
    }
    if let Some(tab) = moved {
        let dst = wins.entry(to.to_string()).or_default();
        dst.tabs.push(tab);
        dst.active = Some(id);
    }
    empty
}

fn after_change(app: &AppHandle, label: &str, empty: bool) {
    if empty {
        // Last tab closed: close the window (closing "main" quits the app).
        if let Some(w) = app.get_window(label) {
            let _ = w.close();
        }
        return;
    }
    refresh(app, label);
}

#[tauri::command]
async fn get_state(app: AppHandle, window: Window) -> Snapshot {
    snapshot(&app, window.label())
}

#[tauri::command]
async fn new_tab(app: AppHandle, window: Window) -> Result<(), String> {
    add_tab(&app, window.label(), None)
}

#[tauri::command]
async fn select_tab(app: AppHandle, window: Window, id: u32) -> Result<(), String> {
    let label = window.label().to_string();
    {
        let st = app.state::<AppState>();
        let mut wins = st.wins.lock().unwrap();
        if let Some(ws) = wins.get_mut(&label) {
            if ws.tabs.iter().any(|t| t.id == id) {
                ws.active = Some(id);
            }
        }
    }
    refresh(&app, &label);
    Ok(())
}

#[tauri::command]
async fn close_tab(app: AppHandle, window: Window, id: u32) -> Result<(), String> {
    let label = window.label().to_string();
    if let Some(wv) = app.get_webview(&tab_label(id)) {
        let _ = wv.close();
    }
    let empty = {
        let st = app.state::<AppState>();
        let mut wins = st.wins.lock().unwrap();
        let ws = wins.entry(label.clone()).or_default();
        ws.tabs.retain(|t| t.id != id);
        if ws.active == Some(id) {
            ws.active = ws.tabs.last().map(|t| t.id);
        }
        ws.tabs.is_empty()
    };
    after_change(&app, &label, empty);
    Ok(())
}

#[tauri::command]
async fn detach_tab(app: AppHandle, window: Window, id: u32) -> Result<(), String> {
    let src = window.label().to_string();
    if snapshot(&app, &src).tabs.len() <= 1 {
        return Ok(());
    }
    let label = format!("win-{}", next_id(&app));
    let new_win = make_window(&app, &label, "Pi Connect").map_err(|e| e.to_string())?;
    let wv = app.get_webview(&tab_label(id)).ok_or("tab not found")?;
    wv.reparent(&new_win).map_err(|e| e.to_string())?;
    let empty = move_tab_state(&app, &src, &label, id);
    after_change(&app, &src, empty);
    refresh(&app, &label);
    relayout_later(&app, vec![src, label]);
    Ok(())
}

#[tauri::command]
async fn attach_tab(app: AppHandle, window: Window, id: u32) -> Result<(), String> {
    let src = window.label().to_string();
    if src == "main" {
        return Ok(());
    }
    let main = app.get_window("main").ok_or("main window not found")?;
    let wv = app.get_webview(&tab_label(id)).ok_or("tab not found")?;
    wv.reparent(&main).map_err(|e| e.to_string())?;
    let empty = move_tab_state(&app, &src, "main", id);
    after_change(&app, &src, empty);
    refresh(&app, "main");
    let _ = main.set_focus();
    relayout_later(&app, vec!["main".to_string()]);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_state, new_tab, select_tab, close_tab, detach_tab, attach_tab
        ])
        .on_window_event(|window, event| match event {
            WindowEvent::Resized(_) => layout(window.app_handle(), window),
            WindowEvent::Destroyed => {
                let st = window.state::<AppState>();
                st.wins.lock().unwrap().remove(window.label());
                if window.label() == "main" {
                    window.app_handle().exit(0);
                }
            }
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            make_window(&handle, "main", "Pi Connect")?;
            add_tab(&handle, "main", None)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Pi Connect");
}