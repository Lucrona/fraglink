//! At-rest protection for the device token in the local settings table.
//!
//! On Windows the token is encrypted with DPAPI (`CryptProtectData`, current
//! user scope): the stored blob only decrypts for the same Windows account on
//! the same PC, so copying `fraglink.db` elsewhere doesn't leak the token.
//! Elsewhere (Linux dev/preview builds) it is stored as-is, marked `plain:`.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

/// Encode a token for storage.
pub fn protect(token: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(blob) = dpapi::protect(token.as_bytes()) {
            return format!("dpapi:{}", B64.encode(blob));
        }
    }
    format!("plain:{}", B64.encode(token.as_bytes()))
}

/// Decode a stored token. `None` if it is corrupt or can't be decrypted
/// (e.g. the database was copied to another PC or Windows account).
pub fn unprotect(stored: &str) -> Option<String> {
    if let Some(b) = stored.strip_prefix("plain:") {
        return String::from_utf8(B64.decode(b).ok()?).ok();
    }
    #[cfg(windows)]
    if let Some(b) = stored.strip_prefix("dpapi:") {
        return String::from_utf8(dpapi::unprotect(&B64.decode(b).ok()?)?).ok();
    }
    None
}

#[cfg(windows)]
mod dpapi {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    const ENTROPY: &[u8] = b"fraglink-device-token-v1";

    fn blob(d: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: d.len() as u32,
            pbData: d.as_ptr() as *mut u8,
        }
    }

    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        v
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        // SAFETY: all pointers are valid for the duration of the call; the output
        // buffer is allocated by Windows and released with LocalFree in `take`.
        let ok = unsafe {
            CryptProtectData(
                &input,
                null(),
                &entropy,
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        (ok != 0).then(|| unsafe { take(out) })
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        // SAFETY: as above.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                null_mut(),
                &entropy,
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        (ok != 0).then(|| unsafe { take(out) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = "abc_DEF-123";
        let s = protect(t);
        assert!(!s.contains(t), "token must not be stored verbatim: {s}");
        #[cfg(windows)]
        assert!(s.starts_with("dpapi:"), "{s}");
        assert_eq!(unprotect(&s).as_deref(), Some(t));
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(unprotect("nonsense"), None);
        assert_eq!(unprotect("plain:!!!"), None);
        assert_eq!(unprotect("dpapi:AAAA"), None);
    }
}
