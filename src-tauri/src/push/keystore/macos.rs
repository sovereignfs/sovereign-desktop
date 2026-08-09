//! macOS Keychain storage for the push encryption private key, via
//! `security-framework`'s cross-platform `set_generic_password`/
//! `get_generic_password` (works on macOS and iOS; only macOS is relevant
//! here). A generic password is the right primitive: this is 32 opaque
//! bytes with no certificate/identity semantics, not a TLS credential.

use p256::SecretKey;
use security_framework::passwords::{get_generic_password, set_generic_password};

const SERVICE: &str = "fs.sovereign.desktop.push";
const ACCOUNT: &str = "push-private-key";

/// Loads the stored keypair if one exists, generating and persisting a new
/// one otherwise — the same "get-or-create" shape
/// `sovereign-mobile`'s `PushKeychain.swift`/`PushKeystore.java` use.
pub fn get_or_create_keypair() -> Result<SecretKey, String> {
    match get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => {
            SecretKey::from_slice(&bytes).map_err(|e| format!("stored key is corrupt: {e}"))
        }
        Err(_) => {
            let key = SecretKey::random(&mut rand::thread_rng());
            let bytes = key.to_bytes();
            set_generic_password(SERVICE, ACCOUNT, bytes.as_slice())
                .map_err(|e| format!("failed to store new key in Keychain: {e}"))?;
            Ok(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Keychain round-trip, not mocked — this repo's own dev machine
    /// has a real macOS Keychain to write to. Uses a distinct
    /// service/account from production so a test run never touches (or
    /// collides with) a real stored key.
    #[test]
    fn stores_and_retrieves_a_real_key_via_the_real_keychain() {
        const TEST_SERVICE: &str = "fs.sovereign.desktop.push.test";
        const TEST_ACCOUNT: &str = "push-private-key-test";

        let _ = security_framework::passwords::delete_generic_password(TEST_SERVICE, TEST_ACCOUNT);

        let key = SecretKey::random(&mut rand::thread_rng());
        let bytes = key.to_bytes();
        set_generic_password(TEST_SERVICE, TEST_ACCOUNT, bytes.as_slice()).unwrap();

        let loaded_bytes = get_generic_password(TEST_SERVICE, TEST_ACCOUNT).unwrap();
        let loaded_key = SecretKey::from_slice(&loaded_bytes).unwrap();

        assert_eq!(loaded_key.to_bytes(), key.to_bytes());

        security_framework::passwords::delete_generic_password(TEST_SERVICE, TEST_ACCOUNT)
            .unwrap();
    }
}
