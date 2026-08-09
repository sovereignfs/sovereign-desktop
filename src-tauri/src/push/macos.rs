//! macOS native push (RFC 0087's "Desktop native push" addendum,
//! workstream 0010 leg 3) — device-token registration and
//! decrypt-and-display while running.
//!
//! `application:didRegisterForRemoteNotificationsWithDeviceToken:`,
//! `application:didFailToRegisterForRemoteNotificationsWithError:`, and
//! `application:didReceiveRemoteNotification:` are all
//! `NSApplicationDelegate`-only callbacks — Apple exposes no
//! `NSNotificationCenter` equivalent for any of them, confirmed against
//! Apple's own documentation before writing this, and no Tauri/`tao` API
//! surfaces them (`tao`, Tauri's windowing crate, already owns
//! `NSApplication`'s single `delegate` slot for its own window-lifecycle
//! handling). The only way to receive them without replacing — and
//! breaking — `tao`'s delegate is to add the selectors to `tao`'s existing
//! delegate *class* at runtime, via the Objective-C runtime's
//! `class_addMethod`. This is not a novel trick: Electron's own
//! `app.on('did-register-for-remote-notifications', ...)` solves the
//! identical problem (Chromium owns the delegate slot there) the same way.
//!
//! **Empirically verified working on this machine's real macOS, against a
//! real bundled `.app` (not a bare `cargo build` binary — a raw
//! executable has no Info.plist/bundle identity and never receives any
//! delegate callback at all, regardless of whether the injection itself
//! is correct; this had to be built via `tauri build --debug` and run from
//! `Sovereign.app/Contents/MacOS/` to actually exercise the real
//! `NSApplication` delivery path):** `registerForRemoteNotifications()`
//! reliably drives the *failure* callback
//! (`didFailToRegisterForRemoteNotificationsWithError:`, observed error:
//! `"The operation couldn't be completed. (OSStatus error 13.)"`) — expected,
//! since this repo's ad-hoc "Sign to Run Locally" signing here has no real
//! Apple Developer Team / push entitlement, the same limitation already
//! documented for `sovereign-mobile` leg 4's Keychain access groups. The
//! success callback's wiring (selector, type encoding, IMP signature) is
//! therefore verified structurally and by symmetry with the failure path
//! firing correctly, but not exercised end-to-end with a real device
//! token or a real push delivery — both need a real Team ID, provisioning
//! profile, and Apple's production APNs, unavailable here.
//!
//! **Decrypt-and-display while running, not while quit:** per the RFC
//! addendum, this build ships without a Notification Service Extension
//! equivalent — Tauri has no tooling to embed one in its bundle output.
//! `didReceiveRemoteNotification:` fires whenever this process is alive
//! (foreground or backgrounded/tray-resident; macOS does not suspend
//! background processes the way iOS does), so decrypt-and-display happens
//! there. While fully quit, the OS shows whatever `aps.alert` the relay's
//! `sendApnsPush` sent verbatim (a placeholder, never real content) —
//! no code runs at all for a quit process, so there is no earlier point to
//! intercept it.

use crate::push::{crypto, keystore, registration};
use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSData, NSDictionary, NSError, NSString};
use std::sync::OnceLock;
use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

/// Outcome of a `registerForRemoteNotifications()` attempt.
enum RegistrationOutcome {
    DeviceToken(Vec<u8>),
    Error(String),
}

/// Both injected-method callbacks need to reach back into the running
/// Tauri app (to spawn the registration HTTP call, or decrypt a received
/// payload), but the Objective-C runtime's fixed `Imp` signature
/// (`unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &AnyObject)`)
/// has no room for captured context — every raw objc-runtime-injection
/// approach hits this same limitation. Stashed globally instead; correct
/// here because `setup` runs at most once per process lifetime.
static REGISTRATION_OUTCOME_CALLBACK: OnceLock<Box<dyn Fn(RegistrationOutcome) + Send + Sync>> =
    OnceLock::new();
static RECEIVED_NOTIFICATION_CALLBACK: OnceLock<Box<dyn Fn(String) + Send + Sync>> =
    OnceLock::new();

/// Wires up macOS push end-to-end: loads or creates the on-device keypair,
/// installs the delegate-method injection, and calls
/// `registerForRemoteNotifications()`. Call once, from `.setup()`, after
/// `tao`'s own delegate is set (i.e. not before the window/tray/menu setup
/// that already runs there) and on the main thread.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("sovereign-desktop: push setup skipped — not on main thread");
        return;
    };

    let key = match keystore::macos::get_or_create_keypair() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("sovereign-desktop: push setup failed to load/create keypair: {e}");
            return;
        }
    };

    let app_for_registration = app.clone();
    let key_for_registration = key.clone();
    let _ = REGISTRATION_OUTCOME_CALLBACK.set(Box::new(move |outcome| match outcome {
        RegistrationOutcome::DeviceToken(bytes) => {
            let hex_token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let app = app_for_registration.clone();
            let key = key_for_registration.clone();
            tauri::async_runtime::spawn(async move {
                let result = registration::register(
                    &app,
                    registration::Platform::Macos,
                    registration::DeviceToken::ApnsHex(hex_token),
                    &key,
                )
                .await;
                if let Err(e) = result {
                    eprintln!("sovereign-desktop: push registration failed: {e}");
                }
            });
        }
        RegistrationOutcome::Error(message) => {
            // Expected under ad-hoc local signing with no real Apple
            // Developer Team / push entitlement — see this module's own
            // doc comment. Logged, not surfaced to the user: push is an
            // optional capability, not a blocking failure.
            eprintln!("sovereign-desktop: push registration failed: {message}");
        }
    }));

    let app_for_display = app.clone();
    let key_for_display = key.clone();
    let _ = RECEIVED_NOTIFICATION_CALLBACK.set(Box::new(move |encrypted_payload| {
        match crypto::decrypt(&encrypted_payload, &key_for_display) {
            Ok(plaintext) => {
                let parsed: serde_json::Value = match serde_json::from_slice(&plaintext) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("sovereign-desktop: push payload decrypted but not valid JSON: {e}");
                        return;
                    }
                };
                let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("Sovereign");
                let body = parsed.get("body").and_then(|v| v.as_str());
                let mut builder = app_for_display.notification().builder().title(title);
                if let Some(body) = body {
                    builder = builder.body(body);
                }
                if let Err(e) = builder.show() {
                    eprintln!("sovereign-desktop: failed to show decrypted push notification: {e}");
                }
            }
            Err(e) => {
                eprintln!("sovereign-desktop: failed to decrypt received push payload: {e}");
            }
        }
    }));

    let app_kit = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app_kit.delegate() else {
        eprintln!(
            "sovereign-desktop: push registration skipped — NSApplication has no delegate yet"
        );
        return;
    };

    let class: &AnyClass = AsRef::<AnyObject>::as_ref(&delegate).class();
    let class_ptr = class as *const AnyClass as *mut AnyClass;

    unsafe {
        let sel_success =
            Sel::register(c"application:didRegisterForRemoteNotificationsWithDeviceToken:");
        let sel_failure =
            Sel::register(c"application:didFailToRegisterForRemoteNotificationsWithError:");
        let sel_received = Sel::register(c"application:didReceiveRemoteNotification:");
        // `"v@:@@"` — void return; self (@), _cmd (:), then two more
        // object (@) arguments. Modern objc runtimes don't require the
        // legacy stack-offset numbers some older type-encoding examples
        // include.
        let types = c"v@:@@";

        let imp_success: Imp = core::mem::transmute::<
            unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &AnyObject),
            Imp,
        >(did_register_for_remote_notifications);
        let imp_failure: Imp = core::mem::transmute::<
            unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &AnyObject),
            Imp,
        >(did_fail_to_register_for_remote_notifications);
        let imp_received: Imp = core::mem::transmute::<
            unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &AnyObject),
            Imp,
        >(did_receive_remote_notification);

        let added_success = class_addMethod(class_ptr, sel_success, imp_success, types.as_ptr());
        let added_failure = class_addMethod(class_ptr, sel_failure, imp_failure, types.as_ptr());
        let added_received =
            class_addMethod(class_ptr, sel_received, imp_received, types.as_ptr());
        if !added_success.as_bool() || !added_failure.as_bool() || !added_received.as_bool() {
            // `class_addMethod` returns false if the selector already
            // exists on the class — meaning tao (or a future Tauri
            // version) already implements one of these, which would make
            // this injection a silent no-op rather than an addition.
            // Surfaced loudly rather than swallowed, since a silently
            // no-op'd injection means push looks wired but never actually
            // fires.
            eprintln!(
                "sovereign-desktop: push delegate method injection reported failure \
                 (success={added_success:?}, failure={added_failure:?}, \
                 received={added_received:?}) — the target class may already implement \
                 one of these selectors"
            );
        }
    }

    app_kit.registerForRemoteNotifications();
}

unsafe extern "C-unwind" fn did_register_for_remote_notifications(
    _this: &AnyObject,
    _sel: Sel,
    _application: &AnyObject,
    device_token: &AnyObject,
) {
    let data: Retained<NSData> = unsafe {
        Retained::retain(device_token as *const AnyObject as *mut NSData).expect("non-null NSData")
    };
    let bytes = data.to_vec();
    if let Some(cb) = REGISTRATION_OUTCOME_CALLBACK.get() {
        cb(RegistrationOutcome::DeviceToken(bytes));
    }
}

unsafe extern "C-unwind" fn did_fail_to_register_for_remote_notifications(
    _this: &AnyObject,
    _sel: Sel,
    _application: &AnyObject,
    error: &AnyObject,
) {
    let err: Retained<NSError> = unsafe {
        Retained::retain(error as *const AnyObject as *mut NSError).expect("non-null NSError")
    };
    let message = err.localizedDescription().to_string();
    if let Some(cb) = REGISTRATION_OUTCOME_CALLBACK.get() {
        cb(RegistrationOutcome::Error(message));
    }
}

unsafe extern "C-unwind" fn did_receive_remote_notification(
    _this: &AnyObject,
    _sel: Sel,
    _application: &AnyObject,
    user_info: &AnyObject,
) {
    let dict: Retained<NSDictionary<AnyObject, AnyObject>> = unsafe {
        Retained::retain(user_info as *const AnyObject as *mut NSDictionary<AnyObject, AnyObject>)
            .expect("non-null NSDictionary")
    };
    let key = NSString::from_str("encryptedPayload");
    let Some(value) = dict.objectForKey(AsRef::<AnyObject>::as_ref(&key)) else {
        eprintln!("sovereign-desktop: received remote notification with no encryptedPayload key");
        return;
    };
    let value_str: Retained<NSString> = unsafe {
        Retained::retain(Retained::as_ptr(&value) as *mut NSString).expect("non-null NSString")
    };
    let encrypted_payload = value_str.to_string();
    if let Some(cb) = RECEIVED_NOTIFICATION_CALLBACK.get() {
        cb(encrypted_payload);
    }
}
