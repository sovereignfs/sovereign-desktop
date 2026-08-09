//! The Tauri transport of `@sovereignfs/bridge` (RFC 0083, workstream 0003
//! leg 3) — a single narrow command, `bridge_invoke`, that the injected
//! `window.__SOVEREIGN_BRIDGE__` object (see `bridge_script()` below) calls
//! for every `sdk.device.*` capability call. Real native delivery via
//! `tauri-plugin-notification`'s `NotificationExt`, not a `window.Notification`
//! shim — see this leg's PR description for why that distinction mattered.
//!
//! v1 implements `notifications.native`, `camera.photo`, and
//! `biometrics.confirm`. `haptics.impact` is deliberately absent from both
//! the advertised `capabilities` list and this dispatch — RFC 0083 §7
//! specifies it as a Tauri no-op (`unavailable`), so omitting it here lets
//! `@sovereignfs/bridge`'s own "no native shell answers this capability"
//! path handle it, exactly as it already does for a plain browser with no
//! Vibration API. Advertising a capability this transport cannot honor would
//! be worse than omitting it.
//!
//! **`camera.photo` is a native file picker only — never live webcam
//! capture.** Desktop hardware makes live capture low-value (most desktops
//! have none; laptop webcams are front-facing and awkward for the same
//! "photograph a document" use case mobile's camera solves) and
//! meaningfully more complex to build (permission prompts, a live preview
//! surface). `tauri-plugin-dialog`'s native file picker is already a
//! dependency (the auto-updater's prompt) and gives real utility for the
//! same `DeviceResult<{ dataUrl, mimeType }>` contract mobile's
//! `Bridge.swift`/`BridgeCapabilities.java` return. The `source: 'camera' |
//! 'library'` distinction the SDK sends is intentionally ignored — both
//! resolve to the same picker, since there is no separate "camera" mode to
//! route to.
//!
//! **`biometrics.confirm` (epic task 17.10) dispatches to `crate::biometrics`
//! — see that module's doc comment for the macOS/Windows/Linux split and
//! what is and isn't actually verified on each.**

use base64::prelude::*;
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

/// Mirrors `@sovereignfs/sdk/device-bridge`'s `DeviceResult<T>` discriminated
/// union exactly — same four non-`ok` variants, same field names — so the
/// injected bridge script can hand this straight back to
/// `BridgeImpl.invoke()`'s caller with no reshaping.
pub(crate) fn ok(value: Value) -> Value {
    json!({ "status": "ok", "value": value })
}

pub(crate) fn unavailable(capability: &str) -> Value {
    json!({ "status": "unavailable", "capability": capability })
}

pub(crate) fn dismissed() -> Value {
    json!({ "status": "dismissed" })
}

pub(crate) fn denied() -> Value {
    json!({ "status": "denied" })
}

pub(crate) fn failed(error: impl std::fmt::Display) -> Value {
    json!({ "status": "failed", "error": error.to_string() })
}

/// Dispatches one `sdk.device.*` capability call. `payload`'s shape is
/// per-capability; `notifications.native` uses `{ title: string, body?:
/// string }`, matching `nativeNotifications.show()`'s input type verbatim.
/// `camera.photo` uses `{ source: 'camera' | 'library' }`, ignored (see
/// module doc comment). `biometrics.confirm` uses `{ reason?: string }`.
#[tauri::command]
pub fn bridge_invoke<R: Runtime>(app: AppHandle<R>, capability: String, payload: Value) -> Value {
    match capability.as_str() {
        "notifications.native" => notify(&app, &payload),
        "camera.photo" => camera_photo(&app),
        "biometrics.confirm" => {
            let reason = payload
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("Confirm it's you");
            crate::biometrics::confirm(reason)
        }
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

fn camera_photo<R: Runtime>(app: &AppHandle<R>) -> Value {
    let picked = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .blocking_pick_file();

    let Some(file_path) = picked else {
        // No distinct cancel signal from the OS dialog beyond `None` — same
        // "cancel reads as the user backing out" reasoning as
        // `biometrics.confirm`'s `.userCancel` case on mobile.
        return dismissed();
    };

    let path = match file_path.into_path() {
        Ok(p) => p,
        Err(e) => return failed(e),
    };

    let Some(mime_type) = mime_type_for_extension(&path) else {
        return failed("unsupported image file type");
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return failed(e),
    };

    ok(json!({
        "dataUrl": format!("data:{mime_type};base64,{}", BASE64_STANDARD.encode(&bytes)),
        "mimeType": mime_type,
    }))
}

/// Extracted as a pure function so the extension → MIME mapping is
/// unit-testable without a real file dialog (which needs GUI interaction
/// this environment cannot drive headlessly).
fn mime_type_for_extension(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_type_for_extension_matches_known_image_types() {
        assert_eq!(
            mime_type_for_extension(Path::new("photo.png")),
            Some("image/png")
        );
        assert_eq!(
            mime_type_for_extension(Path::new("photo.jpg")),
            Some("image/jpeg")
        );
        assert_eq!(
            mime_type_for_extension(Path::new("photo.JPEG")),
            Some("image/jpeg")
        );
        assert_eq!(
            mime_type_for_extension(Path::new("photo.gif")),
            Some("image/gif")
        );
        assert_eq!(
            mime_type_for_extension(Path::new("photo.webp")),
            Some("image/webp")
        );
    }

    #[test]
    fn mime_type_for_extension_rejects_unknown_or_missing_extension() {
        assert_eq!(mime_type_for_extension(Path::new("document.pdf")), None);
        assert_eq!(mime_type_for_extension(Path::new("noextension")), None);
    }
}
