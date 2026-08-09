//! Windows native push (RFC 0087's "Desktop native push" addendum,
//! workstream 0010 leg 3) — channel registration and decrypt-and-display
//! while running, via WinRT's `PushNotificationChannelManager`.
//!
//! Unlike macOS, this needs no delegate-injection trick: WinRT's
//! `PushNotificationChannel::PushNotificationReceived` is a real, official
//! event subscription API, and it only fires while this process is alive —
//! which is exactly the "raw-only, running-app-only" behavior the RFC
//! addendum settled on for Windows (see `apps/relay/src/wns.ts` in the
//! `sovereign` monorepo for why: WNS toast notifications could show a
//! closed-app banner, but only by sending Microsoft plaintext content,
//! which conflicts with this RFC's content-blind guarantee).
//!
//! **Written but cross-compile-type-checked only** (`cargo check --target
//! x86_64-pc-windows-msvc` against an isolated probe crate exercising this
//! exact API surface — channel creation, `PushNotificationReceived`
//! subscription, `RawNotification::Content()` — since the full
//! `sovereign-desktop` binary can't be cross-checked here; see
//! `crate::push::keystore::windows`'s doc comment for why). Do not treat
//! this as more verified than that until someone builds and runs it on
//! real Windows.
//!
//! `CreatePushNotificationChannelForApplicationAsync()` requires the
//! unpackaged process to have an associated app identity (Partner
//! Center-issued Package SID) — the exact current API for that
//! association on an unpackaged Win32 app needs verification during real
//! Windows testing, per RFC 0087's addendum's own open questions; nothing
//! in this file attempts that association itself, since the WinRT API
//! surface used here doesn't expose it directly (it's expected to be
//! resolved via the process's own identity at the OS level, not a Rust
//! API call).

use crate::push::{crypto, keystore, registration};
use tauri::{AppHandle, Runtime};
use windows::core::HSTRING;
use windows::Foundation::TypedEventHandler;
use windows::Networking::PushNotifications::{
    PushNotificationChannel, PushNotificationChannelManager, PushNotificationReceivedEventArgs,
};

/// Wires up Windows push end-to-end: loads or creates the on-device
/// keypair, creates a push notification channel (a blocking WinRT call,
/// run on a dedicated OS thread so it never blocks Tauri's `.setup()`),
/// subscribes to `PushNotificationReceived` for decrypt-and-display, and
/// registers the channel URI with the active instance.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        let key = match keystore::windows::get_or_create_keypair() {
            Ok(k) => k,
            Err(e) => {
                eprintln!("sovereign-desktop: push setup failed to load/create keypair: {e}");
                return;
            }
        };

        let channel = match create_channel() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("sovereign-desktop: push channel creation failed: {e:?}");
                return;
            }
        };

        let key_for_display = key.clone();
        let subscribe_result = channel.PushNotificationReceived(&TypedEventHandler::<
            PushNotificationChannel,
            PushNotificationReceivedEventArgs,
        >::new(move |_sender, args| {
            let Some(args) = args else { return Ok(()) };
            let raw = args.RawNotification()?;
            let content: HSTRING = raw.Content()?;
            decrypt_and_display(&app, &key_for_display, &content.to_string());
            Ok(())
        }));
        if let Err(e) = subscribe_result {
            eprintln!("sovereign-desktop: failed to subscribe to PushNotificationReceived: {e:?}");
        }

        let uri = match channel.Uri() {
            Ok(u) => u.to_string(),
            Err(e) => {
                eprintln!("sovereign-desktop: failed to read channel URI: {e:?}");
                return;
            }
        };

        let result = tauri::async_runtime::block_on(registration::register(
            &app,
            registration::Platform::Windows,
            registration::DeviceToken::WnsChannelUri(uri),
            &key,
        ));
        if let Err(e) = result {
            eprintln!("sovereign-desktop: push registration failed: {e}");
        }
    });
}

fn create_channel() -> windows::core::Result<PushNotificationChannel> {
    let op = PushNotificationChannelManager::CreatePushNotificationChannelForApplicationAsync()?;
    op.get()
}

fn decrypt_and_display<R: Runtime>(app: &AppHandle<R>, key: &p256::SecretKey, encrypted_payload: &str) {
    use tauri_plugin_notification::NotificationExt;

    let plaintext = match crypto::decrypt(encrypted_payload, key) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sovereign-desktop: failed to decrypt received push payload: {e}");
            return;
        }
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&plaintext) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sovereign-desktop: push payload decrypted but not valid JSON: {e}");
            return;
        }
    };
    let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("Sovereign");
    let body = parsed.get("body").and_then(|v| v.as_str());
    let mut builder = app.notification().builder().title(title);
    if let Some(body) = body {
        builder = builder.body(body);
    }
    if let Err(e) = builder.show() {
        eprintln!("sovereign-desktop: failed to show decrypted push notification: {e}");
    }
}
