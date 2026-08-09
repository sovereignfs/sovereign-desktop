//! Native `POST /api/account/push-device-token` registration (RFC 0087's
//! "Desktop native push" addendum, workstream 0010 leg 3) — entirely
//! native Rust, not the bundled onboarding page's JS and not routed
//! through `bridge.json`'s remote grant, mirroring `sovereign-mobile` leg
//! 4's "entirely native, zero new bridge capability" decision for the same
//! two reasons: RFC 0083 §7 (`secureStorage`-shaped capabilities must
//! never be plugin-facing) and this repo's own hard rule against widening
//! `bridge.json`'s remote grant beyond `allow-bridge-invoke` both apply
//! here without modification.
//!
//! Reads the active instance URL from the same `tauri-plugin-store`
//! `instances.json` `src/store.ts` already writes (`crate::active_instance_origin`,
//! the exact source `allow_navigation`'s own origin check already uses —
//! one source of truth, not a second copy), and the session cookie from
//! the webview's own cookie jar via `WebviewWindow::cookies_for_url` —
//! never asks the page for either.

use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use tauri::{AppHandle, Manager, Runtime};

/// `'macos' | 'windows'` — the two platforms this leg adds (RFC 0087's
/// addendum). Not `'ios' | 'android'`, which stay `sovereign-mobile`'s own.
pub enum Platform {
    Macos,
    Windows,
}

impl Platform {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

/// `deviceToken` per RFC 0087's addendum: APNs's raw device token for
/// macOS (hex-encoded, matching how APNs device tokens are conventionally
/// represented — the relay's `sendApnsPush` just forwards this string
/// verbatim as the `/3/device/{token}` path segment), or the full WNS
/// channel URI string for Windows (WNS has no separate opaque-token
/// concept — see `apps/relay/src/wns.ts` in the `sovereign` monorepo).
pub enum DeviceToken {
    ApnsHex(String),
    WnsChannelUri(String),
}

impl DeviceToken {
    fn as_wire_string(&self) -> &str {
        match self {
            Self::ApnsHex(s) => s,
            Self::WnsChannelUri(s) => s,
        }
    }
}

/// Registers this device's push encryption public key and native device
/// token with the currently active instance. Silently does nothing (logs
/// to stderr, never panics) when there's no active instance yet — the
/// same "no product features in the shell" posture as everything else
/// here: a fresh install with no instance configured has nothing to
/// register against, and that's a normal state, not an error.
pub async fn register<R: Runtime>(
    app: &AppHandle<R>,
    platform: Platform,
    device_token: DeviceToken,
    private_key: &SecretKey,
) -> Result<(), String> {
    let Some(instance_url) = crate::active_instance_origin(app) else {
        eprintln!("sovereign-desktop: push registration skipped — no active instance configured");
        return Ok(());
    };

    let Some(window) = app.get_webview_window("main") else {
        return Err("no main window to read session cookies from".to_string());
    };
    let cookies = window
        .cookies_for_url(instance_url.clone())
        .map_err(|e| format!("failed to read cookies: {e}"))?;
    let cookie_header = cookies
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");

    let public_key_b64 = {
        use base64::prelude::*;
        BASE64_STANDARD.encode(private_key.public_key().to_encoded_point(false).as_bytes())
    };

    let endpoint = instance_url
        .join("/api/account/push-device-token")
        .map_err(|e| format!("failed to build registration URL: {e}"))?;

    let body = serde_json::json!({
        "platform": platform.as_str(),
        "deviceToken": device_token.as_wire_string(),
        "publicKey": public_key_b64,
    })
    .to_string();

    let client = tauri_plugin_http::reqwest::Client::new();
    let response = client
        .post(endpoint)
        .header("cookie", cookie_header)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("registration request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("registration rejected ({status}): {text}"));
    }

    Ok(())
}
