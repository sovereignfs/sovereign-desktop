//! Touch ID via `LocalAuthentication.framework`, called through
//! `objc2-local-authentication`'s generated bindings — the same underlying
//! API sovereign-mobile's `Bridge.swift` calls from Swift, just reached
//! through Rust↔Objective-C FFI instead. See the parent module's doc
//! comment for what is and isn't actually verified here.

use crate::bridge::{denied, dismissed, failed, ok, unavailable};
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use serde_json::Value;
use std::sync::mpsc;

pub fn confirm(reason: &str) -> Value {
    let context = unsafe { LAContext::new() };

    if unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics) }
        .is_err()
    {
        return unavailable("biometrics.confirm");
    }

    // `evaluatePolicy:localizedReason:reply:` is callback-only — Apple never
    // exposes a blocking variant, since evaluation may show UI — so this
    // bridges it into `bridge_invoke`'s synchronous `#[tauri::command]` the
    // same way `tauri-plugin-dialog`'s own `blocking_pick_file()` bridges
    // its callback-based picker: a channel, sent from the reply block,
    // received (blocking) here on the command's own thread.
    let (tx, rx) = mpsc::sync_channel::<(bool, Option<isize>)>(1);
    let reason_ns = NSString::from_str(reason);

    let block = block2::RcBlock::new(move |success: objc2::runtime::Bool, error: *mut NSError| {
        let code = if error.is_null() {
            None
        } else {
            Some(unsafe { &*error }.code() as isize)
        };
        let _ = tx.send((success.as_bool(), code));
    });

    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &reason_ns,
            &block,
        );
    }

    match rx.recv() {
        Ok((true, _)) => ok(Value::Null),
        Ok((false, Some(code))) => map_la_error(code),
        Ok((false, None)) => failed("biometric evaluation failed with no error code"),
        Err(_) => failed("biometric evaluation reply channel closed unexpectedly"),
    }
}

/// Mirrors sovereign-mobile's `Bridge.swift` `LAError` mapping exactly —
/// same codes, same `DeviceResult` outcomes. Verified against
/// `objc2-local-authentication`'s real generated `LAError` constants before
/// writing this (`userCancel: -2`, `userFallback: -3`, `systemCancel: -4`,
/// `appCancel: -9`, `authenticationFailed: -1`, `biometryNotAvailable`/
/// `TouchIDNotAvailable: -6`, `biometryNotEnrolled`/`TouchIDNotEnrolled: -7`,
/// `biometryLockout`/`TouchIDLockout: -8`), not guessed.
fn map_la_error(code: isize) -> Value {
    match code {
        -2 | -3 | -4 | -9 => dismissed(), // userCancel, userFallback, systemCancel, appCancel
        -1 => denied(),                   // authenticationFailed
        -6 | -7 | -8 => unavailable("biometrics.confirm"), // biometryNotAvailable/NotEnrolled/Lockout
        _ => failed(format!("LAError code {code}")),
    }
}
