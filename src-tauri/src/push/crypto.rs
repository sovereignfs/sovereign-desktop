//! ECDH (P-256) + HKDF-SHA256 + AES-256-GCM decrypt, matching RFC 0087's
//! wire format exactly — the same one `sovereign-mobile` leg 4 verified
//! against real Swift/Java decryption: base64 of `[65-byte uncompressed
//! SEC1/X9.63 ephemeral public key] ‖ [12-byte AES-GCM IV] ‖ [16-byte
//! AES-GCM auth tag] ‖ [ciphertext]`. HKDF info string is the fixed
//! `"sovereign-push-v1"`, zero-length salt, 32-byte output.
//!
//! **Byte-order note, carried over from the Android leg (the same gotcha,
//! same fix):** `aes-gcm`'s `Aead::decrypt` expects `ciphertext ‖ tag`
//! (tag appended last) — the opposite of this wire format's `tag ‖
//! ciphertext` (tag first). Bytes are reordered before decrypting, exactly
//! like `PushCrypto.java`'s `decrypt()` had to.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

const EPHEMERAL_PUBLIC_KEY_LEN: usize = 65;
const IV_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HKDF_INFO: &[u8] = b"sovereign-push-v1";

#[derive(Debug)]
pub enum DecryptError {
    Base64(base64::DecodeError),
    TooShort,
    InvalidEphemeralPublicKey,
    Hkdf,
    Aead,
}

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base64(e) => write!(f, "base64 decode failed: {e}"),
            Self::TooShort => write!(f, "wire payload shorter than the fixed-size header"),
            Self::InvalidEphemeralPublicKey => write!(f, "invalid ephemeral public key point"),
            Self::Hkdf => write!(f, "HKDF expansion failed"),
            Self::Aead => write!(f, "AES-256-GCM decryption failed (wrong key or tampered data)"),
        }
    }
}

impl std::error::Error for DecryptError {}

/// Decrypts a push payload using the device's own retained private key.
/// Returns the decrypted JSON payload's raw bytes — parsing that JSON is
/// the caller's job, not this function's.
pub fn decrypt(wire_base64: &str, private_key: &SecretKey) -> Result<Vec<u8>, DecryptError> {
    use base64::prelude::*;
    let wire = BASE64_STANDARD.decode(wire_base64).map_err(DecryptError::Base64)?;

    if wire.len() < EPHEMERAL_PUBLIC_KEY_LEN + IV_LEN + TAG_LEN {
        return Err(DecryptError::TooShort);
    }

    let (ephemeral_public_key_bytes, rest) = wire.split_at(EPHEMERAL_PUBLIC_KEY_LEN);
    let (iv, rest) = rest.split_at(IV_LEN);
    let (tag, ciphertext) = rest.split_at(TAG_LEN);

    let ephemeral_public_key = PublicKey::from_sec1_bytes(ephemeral_public_key_bytes)
        .map_err(|_| DecryptError::InvalidEphemeralPublicKey)?;

    let shared_secret =
        p256::ecdh::diffie_hellman(private_key.to_nonzero_scalar(), ephemeral_public_key.as_affine());

    let hkdf = Hkdf::<Sha256>::new(None, shared_secret.raw_secret_bytes().as_slice());
    let mut derived_key = [0u8; 32];
    hkdf.expand(HKDF_INFO, &mut derived_key).map_err(|_| DecryptError::Hkdf)?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived_key));
    let nonce = Nonce::from_slice(iv);

    // Reorder tag‖ciphertext (wire format) into ciphertext‖tag (what
    // `aes-gcm`'s Aead::decrypt expects) — see this module's doc comment.
    let mut ciphertext_then_tag = Vec::with_capacity(ciphertext.len() + tag.len());
    ciphertext_then_tag.extend_from_slice(ciphertext);
    ciphertext_then_tag.extend_from_slice(tag);

    cipher
        .decrypt(nonce, ciphertext_then_tag.as_slice())
        .map_err(|_| DecryptError::Aead)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::rand_core::OsRng;
    use aes_gcm::AeadInPlace;
    use p256::elliptic_curve::sec1::ToEncodedPoint;

    /// Builds a wire-format payload the way the server (`runtime/src/push-encryption.ts`
    /// in the `sovereign` monorepo) does, so this test exercises the exact
    /// same framing `decrypt` must parse — not a simplified round-trip.
    fn encrypt_for_test(plaintext: &[u8], recipient_public_key: &PublicKey) -> String {
        use base64::prelude::*;

        let ephemeral_secret = SecretKey::random(&mut OsRng);
        let ephemeral_public_key_bytes =
            ephemeral_secret.public_key().to_encoded_point(false).as_bytes().to_vec();
        assert_eq!(ephemeral_public_key_bytes.len(), EPHEMERAL_PUBLIC_KEY_LEN);

        let shared_secret = p256::ecdh::diffie_hellman(
            ephemeral_secret.to_nonzero_scalar(),
            recipient_public_key.as_affine(),
        );
        let hkdf = Hkdf::<Sha256>::new(None, shared_secret.raw_secret_bytes().as_slice());
        let mut derived_key = [0u8; 32];
        hkdf.expand(HKDF_INFO, &mut derived_key).unwrap();

        let mut iv = [0u8; IV_LEN];
        aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut iv);

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived_key));
        let mut buffer = plaintext.to_vec();
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(&iv), b"", &mut buffer)
            .unwrap();

        let mut wire = Vec::new();
        wire.extend_from_slice(&ephemeral_public_key_bytes);
        wire.extend_from_slice(&iv);
        wire.extend_from_slice(&tag);
        wire.extend_from_slice(&buffer);
        BASE64_STANDARD.encode(&wire)
    }

    #[test]
    fn decrypts_a_payload_encrypted_against_the_recipient_public_key() {
        let recipient_secret = SecretKey::random(&mut OsRng);
        let recipient_public = recipient_secret.public_key();

        let wire = encrypt_for_test(b"{\"title\":\"hi\"}", &recipient_public);
        let plaintext = decrypt(&wire, &recipient_secret).unwrap();

        assert_eq!(plaintext, b"{\"title\":\"hi\"}");
    }

    #[test]
    fn fails_with_the_wrong_private_key() {
        let recipient_secret = SecretKey::random(&mut OsRng);
        let recipient_public = recipient_secret.public_key();
        let wrong_secret = SecretKey::random(&mut OsRng);

        let wire = encrypt_for_test(b"payload", &recipient_public);
        assert!(decrypt(&wire, &wrong_secret).is_err());
    }

    #[test]
    fn fails_on_a_truncated_wire_payload() {
        use base64::prelude::*;
        let recipient_secret = SecretKey::random(&mut OsRng);
        let short = BASE64_STANDARD.encode([0u8; 10]);
        assert!(matches!(decrypt(&short, &recipient_secret), Err(DecryptError::TooShort)));
    }

    #[test]
    fn fails_on_invalid_base64() {
        let recipient_secret = SecretKey::random(&mut OsRng);
        assert!(matches!(
            decrypt("not valid base64!!", &recipient_secret),
            Err(DecryptError::Base64(_))
        ));
    }

    #[test]
    fn fails_on_tampered_ciphertext() {
        use base64::prelude::*;
        let recipient_secret = SecretKey::random(&mut OsRng);
        let recipient_public = recipient_secret.public_key();
        let wire = encrypt_for_test(b"payload", &recipient_public);

        let mut bytes = BASE64_STANDARD.decode(&wire).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let tampered = BASE64_STANDARD.encode(&bytes);

        assert!(decrypt(&tampered, &recipient_secret).is_err());
    }
}
