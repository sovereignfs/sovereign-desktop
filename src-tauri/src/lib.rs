//! Sovereign desktop shell — Tauri entry point.
//!
//! The shell is intentionally thin: all product functionality is served by the
//! user's self-hosted instance. Rust exists only for native glue that must
//! survive the webview navigating to remote content — the application menu and
//! its "Switch Instance…" handler, the shell-detection marker injected into
//! every page, and (RFC 0083, workstream 0003 leg 3) the device bridge.

mod bridge;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;

const SWITCH_INSTANCE_MENU_ID: &str = "switch-instance";
const TRAY_OPEN_MENU_ID: &str = "tray-open";
const TRAY_QUIT_MENU_ID: &str = "tray-quit";

/// Must match `@sovereignfs/bridge`'s `PROTOCOL_VERSION` constant
/// (`packages/bridge/src/protocol.ts` in the `sovereign` monorepo) — a
/// mismatch degrades to the web transport with a console warning rather than
/// breaking (RFC 0083 open question 2), so drifting this is a silent
/// capability loss, not a hard failure, but keep it in sync regardless.
const BRIDGE_PROTOCOL_VERSION: u32 = 1;

/// JavaScript injected into every page load — including the loaded instance —
/// defining `window.__SOVEREIGN_BRIDGE__` per `@sovereignfs/bridge`'s
/// `InstalledBridge` wire shape (`packages/bridge/src/protocol.ts`). Only
/// `notifications.native` is advertised — `haptics.impact` is a deliberate
/// Tauri no-op per RFC 0083 §7, so omitting it here lets the page-side
/// bridge's own "no native shell answers this" path report `unavailable`,
/// exactly like a plain browser with no Vibration API.
///
/// `invoke()` calls the low-level `window.__TAURI_INTERNALS__.invoke(...)` —
/// not `@tauri-apps/api`'s `invoke` wrapper, since this script is a raw
/// string with no bundler — reaching exactly one narrow custom command,
/// `bridge_invoke` (`src/bridge.rs`), which the `bridge` capability grants to
/// the loaded instance's origin. See that capability file's own doc comment
/// for why this is safe: `bridge_invoke` cannot do anything a plugin
/// couldn't already do by calling the standard Web Notifications API in a
/// browser tab.
fn bridge_script() -> String {
    format!(
        "Object.defineProperty(window, '__SOVEREIGN_BRIDGE__', {{ \
             value: Object.freeze({{ \
                 protocolVersion: {protocol_version}, \
                 shell: Object.freeze({{ name: 'sovereign-desktop', version: '{version}', platform: '{platform}' }}), \
                 capabilities: [{{ name: 'notifications.native', version: 1 }}], \
                 invoke: function (capability, payload) {{ \
                     return window.__TAURI_INTERNALS__.invoke('bridge_invoke', {{ capability: capability, payload: payload }}); \
                 }} \
             }}), \
             writable: false, configurable: false, enumerable: true \
         }});",
        protocol_version = BRIDGE_PROTOCOL_VERSION,
        version = env!("CARGO_PKG_VERSION"),
        platform = tauri_platform_name(),
    )
}

/// `BridgeHandshake['shell']['platform']` values are `'ios' | 'android' |
/// 'macos' | 'windows' | 'linux' | 'web'` — a different vocabulary from
/// Rust's `std::env::consts::OS` (`"macos"` matches, but Tauri's own `mobile`
/// cfg and OS constants for the others don't line up 1:1), so this maps
/// explicitly rather than passing `std::env::consts::OS` through.
fn tauri_platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

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
    show_window(&webview);
    let url = format!("{}/?manage=1", app_origin());
    if let Ok(url) = url.parse() {
        let _ = webview.navigate(url);
    }
}

/// Restores the main window from the tray-hidden state (see `run()`'s
/// `CloseRequested` handler) — used by both the tray's "Open" item and the
/// app-menu's "Switch Instance…", since the latter is reachable from the
/// macOS menu bar even while the window is hidden.
fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(webview) = app.get_webview_window("main") {
        show_window(&webview);
    }
}

/// Percent-encodes a `sovereign://` URL for embedding as the bundled page's
/// `?deeplink=` query value.
fn encode_deep_link_param(url: &url::Url) -> String {
    url::form_urlencoded::byte_serialize(url.as_str().as_bytes()).collect()
}

/// Forces the webview back to the local page with the incoming deep link
/// attached as `?deeplink=`, regardless of what it currently shows — mirrors
/// `open_instance_manager`. This is the only reliable fix for the deep-link
/// plugin's own documented macOS race (`deep-link://new-url` can arrive
/// slightly after this app's own `setup()` / initial page load): rather than
/// trying to win the race, the JS side's `main.ts` boot() always defers to a
/// `?deeplink=` param present on *this* page load, so re-navigating here
/// after the fact is sufficient even if the webview had already moved on to
/// a stored instance in the meantime.
fn navigate_to_deep_link<R: Runtime>(app: &AppHandle<R>, incoming: &url::Url) {
    let Some(webview) = app.get_webview_window("main") else {
        return;
    };
    show_window(&webview);
    let target = format!("{}/?deeplink={}", app_origin(), encode_deep_link_param(incoming));
    if let Ok(target) = target.parse() {
        let _ = webview.navigate(target);
    }
}

fn show_window<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![bridge::bridge_invoke])
        .setup(|app| {
            // On Windows/Linux, `sovereign://...` launches a *new* OS process with
            // the URL as its only CLI argument — the deep-link plugin parses that
            // during its own setup, which runs before this closure, so it's already
            // available here. On macOS the URL instead arrives shortly after this
            // closure returns, as a `deep-link://new-url` event (see `on_open_url`
            // below) — `get_current()` is never populated in time for a macOS cold
            // launch, by design of the plugin.
            let initial_deep_link = app
                .deep_link()
                .get_current()
                .ok()
                .flatten()
                .and_then(|urls| urls.into_iter().next());

            // AppImages on Linux (and dev builds on Windows, which have no
            // installer to register the scheme) need runtime registration; macOS
            // and installed Windows/Linux packages register the scheme at
            // bundle/install time from `tauri.conf.json`'s plugin config instead.
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                let _ = app.deep_link().register_all();
            }

            // Fires for every `sovereign://` open while the app is already running,
            // and — on macOS only — for a cold launch too, once the OS delivers it.
            app.deep_link().on_open_url({
                let app_handle = app.handle().clone();
                move |event| {
                    if let Some(url) = event.urls().into_iter().next() {
                        navigate_to_deep_link(&app_handle, &url);
                    }
                }
            });

            // The main window is created here (not in tauri.conf.json) so it can
            // carry the shell-detection and device-bridge initialization
            // scripts. Both run on every navigation, so they're present on the
            // loaded instance too, not just the bundled onboarding page. A
            // Windows/Linux cold-launch deep link is attached as `?deeplink=` on
            // this very first load; `main.ts`'s boot() checks for it before
            // falling back to the stored active instance.
            let initial_path = match &initial_deep_link {
                Some(url) => format!("index.html?deeplink={}", encode_deep_link_param(url)),
                None => "index.html".to_string(),
            };
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(initial_path.into()))
                .title("Sovereign")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 360.0)
                .initialization_script(&desktop_marker_script())
                .initialization_script(&bridge_script())
                .build()?;

            // Closing the window hides it instead of quitting the app — Sovereign
            // keeps running in the tray so notifications and the bridge stay live.
            // `prevent_close()` stops the window (and thus the app) from actually
            // tearing down; "Quit" on the tray menu or the app menu's Cmd+Q are the
            // only ways to fully exit.
            window.on_window_event({
                let window = window.clone();
                move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            });

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

            // System tray — persistent presence so the app is reachable even
            // while the main window is hidden (see the CloseRequested handler
            // above). Mirrors the app menu's items rather than introducing new
            // shell behavior: Open, Switch Instance…, Quit.
            let tray_open = MenuItem::with_id(app, TRAY_OPEN_MENU_ID, "Open", true, None::<&str>)?;
            let tray_switch_instance = MenuItem::with_id(
                app,
                SWITCH_INSTANCE_MENU_ID,
                "Switch Instance…",
                true,
                None::<&str>,
            )?;
            let tray_quit = MenuItem::with_id(app, TRAY_QUIT_MENU_ID, "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(
                app,
                &[
                    &tray_open,
                    &tray_switch_instance,
                    &PredefinedMenuItem::separator(app)?,
                    &tray_quit,
                ],
            )?;
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .ok_or("missing default window icon for tray")?;
            TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .tooltip("Sovereign")
                .build(app)?;

            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == SWITCH_INSTANCE_MENU_ID {
                open_instance_manager(app);
            } else if event.id() == TRAY_OPEN_MENU_ID {
                show_main_window(app);
            } else if event.id() == TRAY_QUIT_MENU_ID {
                app.exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
