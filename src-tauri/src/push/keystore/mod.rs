//! Native credential-store persistence for the push encryption keypair
//! (RFC 0087's "Desktop native push" addendum, workstream 0010 leg 3).
//!
//! Unlike `sovereign-mobile`'s Android leg (constrained to a software P-256
//! key because minSdkVersion 24 predates API 31's Keystore-native EC
//! key-agreement support, and forced to store the public key alongside the
//! private key at generation time since deriving one back out of an opaque
//! Android Keystore private key isn't portable), this desktop key is a
//! plain software `p256::SecretKey` on both platforms — a public key is
//! always derivable from it directly (`SecretKey::public_key()`), so only
//! the private key's raw 32-byte scalar needs to be persisted at all.
//!
//! `service`/`account` naming matches the fixed strings both platforms'
//! implementations use as their credential-store key — see each platform
//! module for the actual constants.

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
