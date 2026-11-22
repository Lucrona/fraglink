//! Keep existing Windows installs working if the main exe name ever changes.
//!
//! 0.3.0-beta.10.1 already ships `fraglink.exe` (confirmed by listing the
//! published NSIS installer). This module is a safety net for any older name
//! (`fraglink-desktop.exe`) left behind in the install folder or in the
//! autostart Run key. Shortcuts are retargeted by the NSIS updater
//! (`OldMainBinaryName` in installer.nsi). We never delete `Fraglink.exe` when
//! the new name is `fraglink.exe`: on Windows those are the same file.

/// Older binary file names that are not the current `fraglink.exe`.
/// `Fraglink.exe` is intentionally omitted (same path as `fraglink.exe`).
pub const STALE_EXE_NAMES: &[&str] = &["fraglink-desktop.exe"];

pub fn is_stale_exe_name(name: &str) -> bool {
    let n = name.rsplit(['/', '\\']).next().unwrap_or(name);
    STALE_EXE_NAMES.iter().any(|s| n.eq_ignore_ascii_case(s))
}

/// A Run-key command should be rewritten when it launches a stale exe from
/// the same install directory as the running binary.
#[cfg(any(test, windows))]
pub fn run_command_needs_rewrite(
    command: &str,
    install_dir: &str,
    new_exe_path: &str,
) -> Option<String> {
    if command.trim().is_empty() {
        return None;
    }
    let lower = command.to_ascii_lowercase();
    if !is_stale_exe_name_in(&lower) {
        return None;
    }
    let dir = install_dir
        .trim_end_matches(['/', '\\'])
        .to_ascii_lowercase();
    if dir.is_empty() || !lower.contains(&dir) {
        return None;
    }
    Some(quote_path(new_exe_path))
}

#[cfg(any(test, windows))]
fn is_stale_exe_name_in(lower_command: &str) -> bool {
    STALE_EXE_NAMES
        .iter()
        .any(|s| lower_command.contains(&s.to_ascii_lowercase()))
}

#[cfg(any(test, windows))]
fn quote_path(path: &str) -> String {
    if path.starts_with('"') {
        path.to_string()
    } else {
        format!("\"{path}\"")
    }
}

/// Drop a leftover stale exe next to us, and (on Windows) rewrite a stale
/// autostart Run entry so the app still starts with Windows.
pub fn apply() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    for name in STALE_EXE_NAMES {
        let leftover = dir.join(name);
        if leftover.exists() && leftover != exe && is_stale_exe_name(name) {
            #[cfg(windows)]
            {
                if let Err(e) = std::fs::remove_file(&leftover) {
                    log::info!(
                        "upgrade: couldn't remove leftover {}: {e}",
                        leftover.display()
                    );
                } else {
                    log::info!("upgrade: removed leftover {name}");
                }
            }
            #[cfg(not(windows))]
            {
                log::debug!("upgrade: leftover {name} would be removed on Windows");
            }
        }
    }
    #[cfg(windows)]
    migrate_run_key(dir, &exe);
}

#[cfg(windows)]
fn migrate_run_key(install_dir: &std::path::Path, new_exe: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_READ,
        KEY_SET_VALUE, REG_SZ,
    };

    // Tauri autostart writes HKCU\...\Run\<productName>, which is "Fraglink".
    // Also check a couple of older/lowercase spellings just in case.
    const VALUE_NAMES: [&str; 3] = ["Fraglink", "fraglink", "fraglink-desktop"];
    let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let mut key = std::ptr::null_mut();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ | KEY_SET_VALUE,
            &mut key,
        )
    };
    if open != ERROR_SUCCESS {
        return;
    }
    let dir = install_dir.to_string_lossy();
    let new_path = new_exe.to_string_lossy();
    for name in VALUE_NAMES {
        let wname = wide(name);
        let mut typ = 0u32;
        let mut size = 0u32;
        let q = unsafe {
            RegQueryValueExW(
                key,
                wname.as_ptr(),
                std::ptr::null_mut(),
                &mut typ,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if q != ERROR_SUCCESS || size == 0 {
            continue;
        }
        let mut buf = vec![0u8; size as usize];
        let q = unsafe {
            RegQueryValueExW(
                key,
                wname.as_ptr(),
                std::ptr::null_mut(),
                &mut typ,
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        if q != ERROR_SUCCESS {
            continue;
        }
        let text = decode_reg_sz(&buf);
        let Some(rewritten) = run_command_needs_rewrite(&text, &dir, &new_path) else {
            continue;
        };
        if rewritten == text {
            continue;
        }
        let data = wide(&rewritten);
        let bytes = (data.len() * 2) as u32;
        let w =
            unsafe { RegSetValueExW(key, wname.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes) };
        if w == ERROR_SUCCESS {
            log::info!(
                "upgrade: rewrote autostart Run\\{name} to {}",
                new_exe.display()
            );
        } else {
            log::info!("upgrade: couldn't rewrite autostart Run\\{name}: {w}");
        }
    }
    unsafe { RegCloseKey(key) };
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn decode_reg_sz(buf: &[u8]) -> String {
    if buf.len() < 2 {
        return String::new();
    }
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
    String::from_utf16_lossy(&u16s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_names() {
        assert!(is_stale_exe_name("fraglink-desktop.exe"));
        assert!(is_stale_exe_name(
            "C:\\\\Users\\\\a\\\\AppData\\\\Local\\\\Fraglink\\\\FRAGLINK-DESKTOP.EXE"
        ));
        assert!(!is_stale_exe_name("fraglink.exe"));
        assert!(!is_stale_exe_name("Fraglink.exe"));
    }

    #[test]
    fn rewrites_run_key_only_for_this_install() {
        let dir = r"C:\Users\Lu\AppData\Local\Fraglink";
        let neu = r"C:\Users\Lu\AppData\Local\Fraglink\fraglink.exe";
        let old = r#""C:\Users\Lu\AppData\Local\Fraglink\fraglink-desktop.exe""#;
        assert_eq!(
            run_command_needs_rewrite(old, dir, neu).as_deref(),
            Some(r#""C:\Users\Lu\AppData\Local\Fraglink\fraglink.exe""#)
        );
        // Different folder: leave it alone.
        assert_eq!(
            run_command_needs_rewrite(old, r"D:\Other\Fraglink", neu),
            None
        );
        // Already the current name.
        assert_eq!(
            run_command_needs_rewrite(
                r#""C:\Users\Lu\AppData\Local\Fraglink\fraglink.exe""#,
                dir,
                neu
            ),
            None
        );
    }
}
