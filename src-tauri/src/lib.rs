//! Sovereign desktop shell — Tauri entry point.
//!
//! The shell is intentionally thin: all product functionality is served by the
//! user's self-hosted instance. Rust exists only for native glue that must
//! survive the webview navigating to remote content — the application menu and
//! its "Switch Instance…" handler, plus the shell-detection marker injected into
//! every page.

use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

const SWITCH_INSTANCE_MENU_ID: &str = "switch-instance";

/// JavaScript injected into every page load — including the loaded instance —
/// before the page's own scripts run. It defines a frozen, read-only
/// `window.__SOVEREIGN_DESKTOP__` marker so the web app (and the SDK's
/// `sdk.device.*` environment detection) can tell it is running inside the
/// desktop shell and enable shell-specific features.
///
/// This is a plain data marker, **not** a bridge to Tauri IPC — remote instance
/// content must never get IPC access. It is safe to expose because it carries
/// no capability, only identifying facts (shell kind, OS, shell version).
fn desktop_marker_script() -> String {
    format!(
        "Object.defineProperty(window, '__SOVEREIGN_DESKTOP__', {{ \
             value: Object.freeze({{ shell: 'desktop', os: '{os}', version: '{version}' }}), \
             writable: false, configurable: false, enumerable: true \
         }});",
        os = std::env::consts::OS,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Origin of the bundled onboarding page. In dev this is the Vite server; in
/// production it is the platform-specific origin Tauri serves app assets from.
fn app_origin() -> &'static str {
    if cfg!(dev) {
        "http://localhost:1420"
    } else if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

fn open_instance_manager<R: Runtime>(app: &AppHandle<R>) {
    let Some(webview) = app.get_webview_window("main") else {
        return;
    };
    let url = format!("{}/?manage=1", app_origin());
    if let Ok(url) = url.parse() {
        let _ = webview.navigate(url);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            // The main window is created here (not in tauri.conf.json) so it can
            // carry the shell-detection initialization script. The script runs on
            // every navigation, so the marker is present on the loaded instance
            // too, not just the bundled onboarding page.
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Sovereign")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 360.0)
                .initialization_script(&desktop_marker_script())
                .build()?;

            // Start from the default menu so the standard app/Edit/Window items
            // survive — without an Edit menu, copy/paste shortcuts do not work
            // inside WKWebView on macOS.
            let menu = Menu::default(app.handle())?;
            let switch_instance = MenuItem::with_id(
                app,
                SWITCH_INSTANCE_MENU_ID,
                "Switch Instance…",
                true,
                Some("CmdOrCtrl+Shift+I"),
            )?;
            let instances = Submenu::with_items(app, "Instances", true, &[&switch_instance])?;
            menu.append(&instances)?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == SWITCH_INSTANCE_MENU_ID {
                open_instance_manager(app);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
