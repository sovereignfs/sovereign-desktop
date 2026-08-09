//! Native biometric confirmation for `biometrics.confirm` (epic task 17.10,
//! RFC 0083) — the desktop counterpart of sovereign-mobile's epic task 20.7.
//! A **local presence confirmation only, never a session or platform-auth
//! grant** — see sovereign-mobile's ADR 0003 (cookie-in-WebView auth); this
//! never touches Sovereign's own auth state, the same scoping note
//! `sdk.device.biometrics.confirm()`'s own doc comment carries
//! (`packages/sdk/src/device-client.ts` in the monorepo).
//!
//! **No Tauri plugin covers this.** `tauri-plugin-biometric`'s own README
//! lists desktop support explicitly as `Linux ✗, Windows ✗, macOS ✗,
//! Android ✓, iOS ✓` — this capability does not exist for desktop anywhere
//! in the Tauri plugin ecosystem, so each OS's native framework is called
//! directly instead.
//!
//! **Verification differs sharply by platform — read this before trusting
//! either implementation blindly:**
//!
//! - **macOS** (`macos.rs`): runtime-verified on this repo's actual dev
//!   machine. A standalone probe binary using the exact same
//!   `LAContext::new()` / `canEvaluatePolicy_error` /
//!   `evaluatePolicy_localizedReason_reply` call sequence compiled cleanly
//!   and, when run, correctly passed `canEvaluatePolicy` (confirming Touch
//!   ID is genuinely available on this machine) and then blocked waiting on
//!   a real interactive OS prompt — the process was killed before
//!   completion rather than clicked through, so the success/error-mapping
//!   path itself is not end-to-end verified, but the FFI plumbing
//!   (`objc2`/`block2` bridging a callback-based ObjC API into a blocking
//!   Rust channel) demonstrably works.
//! - **Windows** (`windows.rs`): only type-checked, via cross-compilation
//!   (`rustup target add x86_64-pc-windows-msvc` +
//!   `cargo check --target x86_64-pc-windows-msvc`, which this repo's CI
//!   does not run) — **never linked, never built as a real `.exe`, never
//!   run.** The API shapes (`UserConsentVerifier`, `UserConsentVerificationResult`,
//!   `UserConsentVerifierAvailability` and their exact variants) were read
//!   from the real generated `windows` crate bindings before writing this,
//!   not guessed, but that is not a substitute for a build+run on actual
//!   Windows. Treat this file as unverified until someone with Windows
//!   access confirms it.
//! - **Linux**: no standard OS-level biometric primitive exists, so this
//!   always reports `unavailable` — the same no-op precedent
//!   `haptics.impact` already established for this transport (RFC 0083 §7).
//!   Not advertised in `lib.rs`'s `capabilities_list()` for the same reason
//!   `haptics.impact` isn't: no point advertising a capability that would
//!   always resolve `unavailable` anyway.

use serde_json::Value;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// `reason` is the localized reason string shown in the OS prompt, matching
/// `sdk.device.biometrics.confirm(reason?)`'s `reason` field.
pub fn confirm(reason: &str) -> Value {
    #[cfg(target_os = "macos")]
    {
        return macos::confirm(reason);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::confirm(reason);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = reason;
        return crate::bridge::unavailable("biometrics.confirm");
    }
}
