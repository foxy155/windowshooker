use crate::theme;

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