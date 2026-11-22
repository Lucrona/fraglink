//! Notification sounds from Options: a friend signing in or out, using the
//! chosen pack (Fraglink, Soft, or none). Played from Rust so they still work
//! when the main window is hidden in the tray.

use tauri::AppHandle;

use crate::friends::setting;
const DEFAULT_SIGNIN: &[u8] = include_bytes!("../sounds/default-signin.wav");
const DEFAULT_SIGNOUT: &[u8] = include_bytes!("../sounds/default-signout.wav");
const SOFT_SIGNIN: &[u8] = include_bytes!("../sounds/soft-signin.wav");
const SOFT_SIGNOUT: &[u8] = include_bytes!("../sounds/soft-signout.wav");

/// Whether this event should make a sound, given the saved Options.
pub fn should_play(kind: &str, signin: bool, signout: bool, pack: &str) -> bool {
    if pack == "none" {
        return false;
    }
    match kind {
        "signin" => signin,
        "signout" => signout,
        _ => false,
    }
}

fn wav_for(kind: &str, pack: &str) -> Option<&'static [u8]> {
    match (pack, kind) {
        ("soft", "signin") => Some(SOFT_SIGNIN),
        ("soft", "signout") => Some(SOFT_SIGNOUT),
        (_, "signin") => Some(DEFAULT_SIGNIN),
        (_, "signout") => Some(DEFAULT_SIGNOUT),
        _ => None,
    }
}

/// Play a sign-in or sign-out sound when the user has that option on.
pub fn maybe_play(app: &AppHandle, kind: &str) {
    // Settings default to off (same as the Options checkboxes).
    let signin = setting(app, "sound_signin").as_deref() == Some("1");
    let signout = setting(app, "sound_signout").as_deref() == Some("1");
    let pack = setting(app, "sound_pack").unwrap_or_else(|| "default".into());
    if !should_play(kind, signin, signout, &pack) {
        return;
    }
    let Some(bytes) = wav_for(kind, &pack) else {
        return;
    };
    play_wav(bytes);
}

fn play_wav(bytes: &'static [u8]) {
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("fraglink-sound".into())
            .spawn(move || unsafe { play_wav_windows(bytes) })
            .ok();
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
    }
}

#[cfg(windows)]
unsafe fn play_wav_windows(bytes: &'static [u8]) {
    use windows_sys::Win32::Media::{PlaySoundA, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    let _ = PlaySoundA(bytes.as_ptr(), 0, SND_MEMORY | SND_NODEFAULT | SND_SYNC);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_toggles_and_pack() {
        assert!(should_play("signin", true, false, "default"));
        assert!(!should_play("signin", false, true, "default"));
        assert!(should_play("signout", false, true, "soft"));
        assert!(!should_play("signin", true, true, "none"));
        assert!(!should_play("request", true, true, "default"));
        assert!(wav_for("signin", "default").unwrap().starts_with(b"RIFF"));
        assert!(wav_for("signout", "soft").unwrap().starts_with(b"RIFF"));
    }
}
