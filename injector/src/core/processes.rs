//! Process enumeration and access probing.
//!
//! Uses `sysinfo` for listing, and raw Win32 for probing whether we can
//! open a handle with the rights the injector needs.

use crate::theme;

// ============================================================
// ACCESS STATUS
// ============================================================

#[derive(Clone, Copy, PartialEq)]
pub enum AccessStatus {
    Injectable,
    LimitedAccess,
    Denied,
}

impl AccessStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AccessStatus::Injectable => "injectable",
            AccessStatus::LimitedAccess => "limited",
            AccessStatus::Denied => "denied",
        }
    }

    pub fn color(&self) -> egui::Color32 {
        match self {
            AccessStatus::Injectable => theme::ok(),
            AccessStatus::LimitedAccess => theme::warn(),
            AccessStatus::Denied => theme::blocked(),
        }
    }
}

#[derive(Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub status: AccessStatus,
}

// ============================================================
// LOOKUP
// ============================================================

pub fn find_processes_by_name(name: &str) -> Vec<ProcessEntry> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let needle = name.to_lowercase();
    let mut out = Vec::new();

    for (pid, process) in sys.processes() {
        let pname = process.name().to_string_lossy().to_string();
        if pname.to_lowercase() == needle || pname.to_lowercase().contains(&needle) {
            let pid_u32 = pid.as_u32();
            let status = probe_access(pid_u32);
            out.push(ProcessEntry {
                pid: pid_u32,
                name: pname,
                status,
            });
        }
    }

    out.sort_by_key(|p| p.pid);
    out
}

pub fn find_process_by_pid(pid: u32) -> Option<ProcessEntry> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    for (spid, process) in sys.processes() {
        if spid.as_u32() == pid {
            let name = process.name().to_string_lossy().to_string();
            let status = probe_access(pid);
            return Some(ProcessEntry { pid, name, status });
        }
    }
    None
}

// ============================================================
// ACCESS PROBING
// ============================================================

#[cfg(windows)]
pub fn probe_access(pid: u32) -> AccessStatus {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    let injection_rights = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_READ
        | PROCESS_VM_WRITE;

    unsafe {
        match OpenProcess(injection_rights, false, pid) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                AccessStatus::Injectable
            }
            Err(_) => match OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                Ok(h) => {
                    let _ = CloseHandle(h);
                    AccessStatus::LimitedAccess
                }
                Err(_) => AccessStatus::Denied,
            },
        }
    }
}

#[cfg(not(windows))]
pub fn probe_access(_pid: u32) -> AccessStatus {
    AccessStatus::Injectable
}

// ============================================================
// PROCESS PICKER
// ============================================================

/// A row in the Memory view's process picker.
#[derive(Clone)]
pub struct ProcessListing {
    pub pid: u32,
    pub name: String,
    /// Category used to colour the small dot next to each row.
    pub category: &'static str,
    /// Whether we can open it with VM read/write rights.
    pub accessible: bool,
}

/// Enumerate every running process, sorted by name. Used by the
/// Memory view's picker modal.
pub fn list_processes_sorted() -> Vec<ProcessListing> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    // Deduplicate by PID. sysinfo occasionally surfaces the same
    // process twice when it refreshes while a process is spawning or
    // exiting. Keep the first entry we see per PID.
    let mut seen_pids: std::collections::HashSet<u32> =
        std::collections::HashSet::new();

    let mut out: Vec<ProcessListing> = sys
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            if !seen_pids.insert(pid_u32) {
                return None;
            }
            let name = process.name().to_string_lossy().to_string();
            let category = categorize(&name);
            let accessible = probe_access(pid_u32) == AccessStatus::Injectable;
            Some(ProcessListing {
                pid: pid_u32,
                name,
                category,
                accessible,
            })
        })
        .collect();

    // Sort: accessible first, then by name.
    out.sort_by(|a, b| {
        b.accessible.cmp(&a.accessible).then_with(|| {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        })
    });
    out
}

fn categorize(name: &str) -> &'static str {
    let n = name.to_lowercase();

    // System processes
    const SYSTEM: &[&str] = &[
        "system",
        "system idle process",
        "registry",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "services.exe",
        "lsass.exe",
        "winlogon.exe",
        "svchost.exe",
        "fontdrvhost.exe",
        "dwm.exe",
        "explorer.exe",
    ];
    if SYSTEM.iter().any(|s| n == *s) {
        return "system";
    }

    // Games / launchers
    const GAME_HINTS: &[&str] = &[
        "steam",
        "epic",
        "battle.net",
        "riot",
        "gog",
        "unity",
        "unreal",
        "game",
        "launcher",
        "bloons",
        "aqw",
        "adventure",
    ];
    if GAME_HINTS.iter().any(|s| n.contains(s)) {
        return "game";
    }

    // Browsers
    const BROWSERS: &[&str] = &[
        "chrome",
        "firefox",
        "msedge",
        "brave",
        "opera",
        "vivaldi",
    ];
    if BROWSERS.iter().any(|s| n.contains(s)) {
        return "browser";
    }

    "app"
}