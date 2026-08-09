//! Native push notifications (RFC 0087's "Desktop native push" addendum,
//! workstream 0010 leg 3) — macOS via APNs, Windows via WNS raw
//! notifications. See `crate::push::macos`'s module doc comment for the
//! macOS device-token registration spike this leg's own workstream doc
//! calls out as the first thing to verify.

pub mod crypto;
pub mod keystore;
pub mod registration;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
