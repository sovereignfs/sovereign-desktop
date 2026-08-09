//! Windows Credential Manager storage for the push encryption private key,
//! via the `windows` crate's `CredWriteW`/`CredReadW` (Win32
//! `Win32_Security_Credentials`). Same "get-or-create" shape as
//! `crate::push::keystore::macos`.
//!
//! **Written but cross-compile-type-checked only** (`cargo check --target
//! x86_64-pc-windows-msvc` against an isolated probe crate, since the full
//! `sovereign-desktop` binary can't be cross-checked here — an unrelated
//! pre-existing dependency, `ring`, needs Windows C headers this machine
//! doesn't have, the same limitation already documented for
//! `src/biometrics/windows.rs`). Do not treat this as more verified than
//! that until someone builds and runs it on real Windows.

use p256::SecretKey;
use windows::core::HSTRING;
use windows::Win32::Security::Credentials::{
    CredFree, CredReadW, CredWriteW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
};

const TARGET_NAME: &str = "fs.sovereign.desktop.push";

/// Loads the stored keypair if one exists, generating and persisting a new
/// one otherwise.
pub fn get_or_create_keypair() -> Result<SecretKey, String> {
    match read_credential(TARGET_NAME) {
        Ok(bytes) => {
            SecretKey::from_slice(&bytes).map_err(|e| format!("stored key is corrupt: {e}"))
        }
        Err(_) => {
            let key = SecretKey::random(&mut rand::thread_rng());
            let bytes = key.to_bytes();
            write_credential(TARGET_NAME, bytes.as_slice())
                .map_err(|e| format!("failed to store new key in Credential Manager: {e:?}"))?;
            Ok(key)
        }
    }
}

fn write_credential(target: &str, secret: &[u8]) -> windows::core::Result<()> {
    let target_w = HSTRING::from(target);
    let mut blob = secret.to_vec();
    let mut cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: windows::core::PWSTR(target_w.as_ptr() as *mut u16),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&mut cred, 0) }
}

fn read_credential(target: &str) -> windows::core::Result<Vec<u8>> {
    let target_w = HSTRING::from(target);
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(&target_w, CRED_TYPE_GENERIC, 0, &mut ptr)?;
        let cred = &*ptr;
        let slice =
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
        let out = slice.to_vec();
        CredFree(ptr as *const _);
        Ok(out)
    }
}
