//! The Tauri transport of `@sovereignfs/bridge` (RFC 0083, workstream 0003
//! leg 3) — a single narrow command, `bridge_invoke`, that the injected
//! `window.__SOVEREIGN_BRIDGE__` object (see `bridge_script()` below) calls
//! for every `sdk.device.*` capability call. Real native delivery via
//! `tauri-plugin-notification`'s `NotificationExt`, not a `window.Notification`
//! shim — see this leg's PR description for why that distinction mattered.
//!
//! v1 implements `notifications.native` only. `haptics.impact` is
//! deliberately absent from both the advertised `capabilities` list and this
//! dispatch — RFC 0083 §7 specifies it as a Tauri no-op (`unavailable`), so
//! omitting it here lets `@sovereignfs/bridge`'s own "no native shell answers
//! this capability" path handle it, exactly as it already does for a plain
//! browser with no Vibration API. Advertising a capability this transport
//! cannot honor would be worse than omitting it.

use serde_json::{json, Value};
use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

/// Mirrors `@sovereignfs/sdk/device-bridge`'s `DeviceResult<T>` discriminated
/// union exactly — same four non-`ok` variants, same field names — so the
/// injected bridge script can hand this straight back to
/// `BridgeImpl.invoke()`'s caller with no reshaping.
fn ok(value: Value) -> Value {
    json!({ "status": "ok", "value": value })
}

fn unavailable(capability: &str) -> Value {
    json!({ "status": "unavailable", "capability": capability })
}

fn failed(error: impl std::fmt::Display) -> Value {
    json!({ "status": "failed", "error": error.to_string() })
}

/// Dispatches one `sdk.device.*` capability call. `payload`'s shape is
/// per-capability; `notifications.native` uses `{ title: string, body?:
/// string }`, matching `nativeNotifications.show()`'s input type verbatim.
#[tauri::command]
pub fn bridge_invoke<R: Runtime>(app: AppHandle<R>, capability: String, payload: Value) -> Value {
    match capability.as_str() {
        "notifications.native" => notify(&app, &payload),
        _ => unavailable(&capability),
    }
}

fn notify<R: Runtime>(app: &AppHandle<R>, payload: &Value) -> Value {
    let Some(title) = payload.get("title").and_then(Value::as_str) else {
        return failed("missing required \"title\" field");
    };
    let mut builder = app.notification().builder().title(title);
    if let Some(body) = payload.get("body").and_then(Value::as_str) {
        builder = builder.body(body);
    }
    match builder.show() {
        Ok(()) => ok(Value::Null),
        Err(e) => failed(e),
    }
}
