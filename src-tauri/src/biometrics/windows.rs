//! Windows Hello via `Windows.Security.Credentials.UI.UserConsentVerifier`
//! (the `windows` crate's WinRT bindings). **Never linked, built as a real
//! `.exe`, or run — only type-checked** via cross-compilation
//! (`cargo check --target x86_64-pc-windows-msvc`, not part of this repo's
//! CI). The API shapes below were read from the real generated bindings
//! (`windows` crate, `Security_Credentials_UI` feature) before writing this,
//! not guessed — but a clean type-check is not proof this actually works on
//! real Windows. Treat this file as unverified until someone with Windows
//! access builds and runs it for real. See the parent module's doc comment
//! for the full verification breakdown across platforms.

use crate::bridge::{denied, dismissed, failed, ok, unavailable};
use serde_json::Value;
use windows::core::HSTRING;
use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};

pub fn confirm(reason: &str) -> Value {
    let availability = match UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()) {
        Ok(a) => a,
        Err(e) => return failed(e),
    };
    if availability != UserConsentVerifierAvailability::Available {
        return unavailable("biometrics.confirm");
    }

    let message = HSTRING::from(reason);
    let result =
        match UserConsentVerifier::RequestVerificationAsync(&message).and_then(|op| op.get()) {
            Ok(r) => r,
            Err(e) => return failed(e),
        };

    map_verification_result(result)
}

/// `UserConsentVerificationResult` has no direct "wrong fingerprint" variant
/// the way macOS's `LAError.authenticationFailed` does — Windows Hello
/// retries internally until it gives up, so `RetriesExhausted` is the
/// closest honest match for `denied` (the user genuinely tried and failed),
/// distinct from `DeviceBusy`'s transient-and-unexpected `failed`.
fn map_verification_result(result: UserConsentVerificationResult) -> Value {
    match result {
        UserConsentVerificationResult::Verified => ok(Value::Null),
        UserConsentVerificationResult::Canceled => dismissed(),
        UserConsentVerificationResult::DeviceNotPresent
        | UserConsentVerificationResult::NotConfiguredForUser
        | UserConsentVerificationResult::DisabledByPolicy => unavailable("biometrics.confirm"),
        UserConsentVerificationResult::RetriesExhausted => denied(),
        UserConsentVerificationResult::DeviceBusy => failed("Windows Hello device busy"),
        _ => failed(format!(
            "unrecognized UserConsentVerificationResult: {}",
            result.0
        )),
    }
}
