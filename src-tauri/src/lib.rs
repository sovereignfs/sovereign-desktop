//! Sovereign desktop shell — Tauri entry point.
//!
//! The shell is intentionally thin: all product functionality is served by the
//! user's self-hosted instance. Rust exists only for native glue that must
//! survive the webview navigating to remote content — the application menu and
//! its "Switch Instance…" handler, the shell-detection marker injected into
//! every page, and (RFC 0083, workstream 0003 leg 3) the device bridge.

mod biometrics;
mod bridge;
mod push;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_store::StoreExt;
use tauri_plugin_updater::UpdaterExt;

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
/// `InstalledBridge` wire shape (`packages/bridge/src/protocol.ts`).
/// `notifications.native` and `camera.photo` are advertised
/// unconditionally; `biometrics.confirm` only on macOS/Windows builds (see
/// `capabilities_list()`) — `haptics.impact` is a deliberate Tauri no-op per
/// RFC 0083 §7, so omitting it here lets the page-side bridge's own "no
/// native shell answers this" path report `unavailable`, exactly like a
/// plain browser with no Vibration API. `biometrics.confirm` on Linux
/// follows the same reasoning: there is no standard OS biometric primitive
/// there (`crate::biometrics`'s doc comment), so it is omitted rather than
/// advertised-then-always-`unavailable`.
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
                 capabilities: [{capabilities}], \
                 invoke: function (capability, payload) {{ \
                     return window.__TAURI_INTERNALS__.invoke('bridge_invoke', {{ capability: capability, payload: payload }}); \
                 }} \
             }}), \
             writable: false, configurable: false, enumerable: true \
         }});",
        protocol_version = BRIDGE_PROTOCOL_VERSION,
        version = env!("CARGO_PKG_VERSION"),
        platform = tauri_platform_name(),
        capabilities = capabilities_list(),
    )
}

/// The `capabilities` array's contents, built at compile time per-platform
/// rather than hand-duplicated across `#[cfg]` branches of `bridge_script()`
/// itself — `biometrics.confirm` is the only entry that varies (macOS and
/// Windows only; see this file's and `crate::biometrics`'s doc comments for
/// why Linux is excluded).
fn capabilities_list() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        "{ name: 'notifications.native', version: 1 }, { name: 'camera.photo', version: 1 }, { name: 'biometrics.confirm', version: 1 }"
    } else {
        "{ name: 'notifications.native', version: 1 }, { name: 'camera.photo', version: 1 }"
    }
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

/// Must match `src/store.ts`'s `STORE_FILE` / `KEY_ACTIVE_URL` — this reads
/// the exact same `tauri-plugin-store` store the TS shell already persists
/// the active instance to, rather than keeping a separate Rust-side copy in
/// sync. Mirrors how mobile's ADR 0007 reads its own native store directly
/// for the same reason.
pub(crate) fn active_instance_origin<R: Runtime>(app: &AppHandle<R>) -> Option<url::Url> {
    let store = app.store("instances.json").ok()?;
    let active_url = store.get("activeUrl")?;
    url::Url::parse(active_url.as_str()?).ok()
}

fn is_local_origin(url: &url::Url) -> bool {
    url::Url::parse(app_origin())
        .map(|local| local.origin() == url.origin())
        .unwrap_or(false)
}

/// The navigation-policy decision itself (epic task 17.8, desktop's
/// counterpart to mobile's ADR 0007 / RFC 0058): the bundled local page is
/// always allowed, a navigation to the currently active instance's own
/// origin is allowed, everything else is denied. Kept as a pure function,
/// separate from `allow_navigation`'s side effect of actually opening a
/// denied URL externally, so it's unit-testable without spawning a real
/// browser process.
fn is_allowed_navigation(url: &url::Url, active_instance: Option<&url::Url>) -> bool {
    is_local_origin(url) || matches!(active_instance, Some(active) if active.origin() == url.origin())
}

/// Registered via `WebviewWindowBuilder::on_navigation`, Tauri's equivalent
/// of iOS's `decidePolicyFor` / Android's `shouldOverrideUrlLoading` — fires
/// for top-level document navigations only (link clicks, redirects, form
/// submits), never for subresource loads like images, scripts, or fetches,
/// so normal instance functionality is unaffected. A denied navigation opens
/// in the system browser instead of silently taking over the shell's
/// WebView.
fn allow_navigation<R: Runtime>(app: &AppHandle<R>, url: &url::Url) -> bool {
    let active = active_instance_origin(app);
    if is_allowed_navigation(url, active.as_ref()) {
        return true;
    }
    let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
    false
}

/// `window.open()` / `target="_blank"` requests go through a *separate* Tauri
/// hook from `on_navigation` above — without registering this one too, such
/// requests silently no-op (Tauri/WRY's own default with nothing configured)
/// rather than following the same policy. Reuses `is_allowed_navigation`
/// directly: same-origin gets a real new window (`Allow`, Tauri's default
/// handling), anything else is denied and reopened in the system browser
/// instead — the same outcome `allow_navigation` reaches for a plain
/// cross-origin link, just via this hook's own response type.
fn handle_new_window_request<R: Runtime>(app: &AppHandle<R>, url: &url::Url) -> NewWindowResponse<R> {
    let active = active_instance_origin(app);
    if is_allowed_navigation(url, active.as_ref()) {
        return NewWindowResponse::Allow;
    }
    let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
    NewWindowResponse::Deny
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> url::Url {
        s.parse().unwrap()
    }

    #[test]
    fn allows_the_bundled_local_page_regardless_of_active_instance() {
        let local = url(&format!("{}/?manage=1", app_origin()));
        assert!(is_allowed_navigation(&local, None));
        assert!(is_allowed_navigation(
            &local,
            Some(&url("https://my.sovereign.example"))
        ));
    }

    #[test]
    fn allows_navigation_matching_the_active_instance_origin() {
        let active = url("https://my.sovereign.example");
        let target = url("https://my.sovereign.example/plugins/console");
        assert!(is_allowed_navigation(&target, Some(&active)));
    }

    #[test]
    fn denies_a_different_origin_than_the_active_instance() {
        let active = url("https://my.sovereign.example");
        let elsewhere = url("https://evil.example/phish");
        assert!(!is_allowed_navigation(&elsewhere, Some(&active)));
    }

    #[test]
    fn denies_everything_non_local_when_no_instance_is_active() {
        let target = url("https://anywhere.example");
        assert!(!is_allowed_navigation(&target, None));
    }

    #[test]
    fn matches_the_active_origin_regardless_of_path() {
        // Same origin, different path/query/fragment on the active instance
        // — e.g. clicking around inside the loaded instance — stays allowed.
        let active = url("https://my.sovereign.example");
        assert!(is_allowed_navigation(
            &url("https://my.sovereign.example/search?q=x#top"),
            Some(&active)
        ));
    }

    #[test]
    fn treats_a_different_port_on_the_same_host_as_a_different_origin() {
        // Matters for local dev instances (http://localhost:3000 etc.) —
        // origin comparison must include the port, not just the hostname.
        let active = url("http://localhost:3000");
        assert!(!is_allowed_navigation(&url("http://localhost:4000"), Some(&active)));
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

/// Epic task 17.5 — checks GitHub Releases (via `tauri.conf.json`'s
/// `plugins.updater.endpoints`) once per launch and, if a newer signed
/// build exists, shows a native "Update available" dialog rather than
/// injecting any UI into the page — the shell's WebView may currently be
/// showing the bundled local page or a loaded instance, and this must work
/// either way without touching either. A native dialog (not a WebView
/// banner) sidesteps that entirely and matches how the rest of this shell
/// keeps product-facing UI out of the page.
///
/// No endpoint reachable, no update available, or the check erroring for
/// any other reason (including an unset/placeholder `pubkey`, before a real
/// signing key is configured) all resolve the same way: nothing happens,
/// silently — matching the review checklist's "no update available → no UI
/// shown".
fn check_for_updates<R: Runtime>(app: &AppHandle<R>) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(updater) = app_handle.updater() else {
            return;
        };
        let Ok(Some(update)) = updater.check().await else {
            return;
        };

        let message = match &update.body {
            Some(notes) if !notes.trim().is_empty() => format!(
                "Sovereign {} is available — you're on {}.\n\n{}",
                update.version, update.current_version, notes
            ),
            _ => format!(
                "Sovereign {} is available — you're on {}.",
                update.version, update.current_version
            ),
        };

        let install_handle = app_handle.clone();
        app_handle
            .dialog()
            .message(message)
            .title("Update available")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Update Now".into(),
                "Later".into(),
            ))
            .show(move |update_now| {
                if !update_now {
                    return;
                }
                tauri::async_runtime::spawn(async move {
                    if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
                        install_handle.restart();
                    }
                });
            });
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![bridge::bridge_invoke])
        .setup(|app| {
            // Native push (workstream 0010 leg 3), run first per this
            // leg's own workstream doc and before window creation since it
            // doesn't depend on it (tao's NSApplicationDelegate is already
            // set by this point).
            #[cfg(target_os = "macos")]
            push::macos::setup(&app.handle().clone());
            #[cfg(target_os = "windows")]
            push::windows::setup(&app.handle().clone());

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
                .on_navigation({
                    let app_handle = app.handle().clone();
                    move |url| allow_navigation(&app_handle, url)
                })
                .on_new_window({
                    let app_handle = app.handle().clone();
                    move |url, _features| handle_new_window_request(&app_handle, &url)
                })
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

            check_for_updates(&app.handle().clone());

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
