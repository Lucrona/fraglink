//! Where the tracker gets its inputs: running processes, user idle time and
//! Steam's RunningAppID. `SystemEnv` reads the real OS; `sim::SimEnv` fakes it.
//!
//! Anti-cheat safety: on Windows we only take a Toolhelp snapshot (exe names)
//! and, for *new* PIDs, open the process with `PROCESS_QUERY_LIMITED_INFORMATION`
//! to read its image path. No memory reads, no injection, no hooks.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Executable file name, e.g. `Wow.exe`.
    pub name: String,
    /// Full image path when it could be read.
    pub path: Option<String>,
}

pub trait Environment: Send {
    fn processes(&mut self) -> Vec<ProcInfo>;
    /// Seconds since the last keyboard/mouse input (0 if unknown).
    fn idle_seconds(&mut self) -> u64;
    /// Steam's RunningAppID, if Steam reports a game running.
    fn steam_running_appid(&mut self) -> Option<u32>;
    /// Short label for logs/UI ("system" or "simulation").
    fn label(&self) -> &'static str;
}

/// The real machine.
#[derive(Default)]
pub struct SystemEnv {
    path_cache: HashMap<u32, (String, Option<String>)>,
}

impl SystemEnv {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve paths only for PIDs we have not seen (or whose name changed).
    fn resolve(&mut self, raw: Vec<(u32, String)>) -> Vec<ProcInfo> {
        let mut out = Vec::with_capacity(raw.len());
        let mut seen = HashMap::with_capacity(raw.len());
        for (pid, name) in raw {
            let path = match self.path_cache.get(&pid) {
                Some((n, p)) if *n == name => p.clone(),
                _ => query_path(pid),
            };
            seen.insert(pid, (name.clone(), path.clone()));
            out.push(ProcInfo { pid, name, path });
        }
        self.path_cache = seen;
        out
    }
}

impl Environment for SystemEnv {
    fn processes(&mut self) -> Vec<ProcInfo> {
        let raw = list_processes();
        self.resolve(raw)
    }

    fn idle_seconds(&mut self) -> u64 {
        idle_seconds()
    }

    fn steam_running_appid(&mut self) -> Option<u32> {
        crate::steam::running_app_id()
    }

    fn label(&self) -> &'static str {
        "system"
    }
}

// ------------------------------------------------------------------ Windows
#[cfg(windows)]
fn list_processes() -> Vec<(u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    // SAFETY: plain Win32 calls with a correctly sized, zeroed PROCESSENTRY32W.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                if e.th32ProcessID != 0 {
                    out.push((e.th32ProcessID, name));
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(windows)]
fn query_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: limited-query handle only; buffer length passed in/out correctly.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(windows)]
pub fn idle_seconds() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    // SAFETY: LASTINPUTINFO is plain data with cbSize set.
    unsafe {
        let mut li = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut li) == 0 {
            return 0;
        }
        let now = GetTickCount();
        (now.wrapping_sub(li.dwTime) / 1000) as u64
    }
}

// ------------------------------------------------------------------ Linux (dev only)
#[cfg(all(not(windows), target_os = "linux"))]
fn list_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let exe_name = std::fs::read_link(e.path().join("exe"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let name = match exe_name {
            Some(n) => n,
            None => match std::fs::read_to_string(e.path().join("comm")) {
                Ok(c) => c.trim().to_string(),
                Err(_) => continue,
            },
        };
        out.push((pid, name));
    }
    out
}

#[cfg(all(not(windows), target_os = "linux"))]
fn query_path(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn list_processes() -> Vec<(u32, String)> {
    Vec::new()
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn query_path(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn idle_seconds() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_env_lists_processes_including_self() {
        let mut env = SystemEnv::new();
        let procs = env.processes();
        if cfg!(any(windows, target_os = "linux")) {
            let me = std::process::id();
            let mine = procs
                .iter()
                .find(|p| p.pid == me)
                .expect("own process listed");
            assert!(!mine.name.is_empty());
            assert!(mine.path.is_some(), "own image path should be readable");
            // second call hits the cache and returns the same info
            let again = env.processes();
            assert_eq!(again.iter().find(|p| p.pid == me), Some(mine));
        }
        let _ = env.idle_seconds();
    }
}
