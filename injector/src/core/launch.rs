use std::thread;
use std::time::{Duration, Instant};

pub fn launch_process(path: &str) -> Result<std::process::Child, String> {
    use std::process::Command;

    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("launch path is empty".into());
    }

    let p = std::path::Path::new(trimmed);
    if !p.exists() {
        return Err(format!("path does not exist: {}", trimmed));
    }

    Command::new(p)
        .spawn()
        .map_err(|e| format!("spawn failed: {}", e))
}

pub fn wait_for_connection(remote_ip: &str, timeout: Duration) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(pid) = find_pid_for_remote(remote_ip) {
            return Some(pid);
        }
        thread::sleep(Duration::from_millis(1500));
    }
    None
}

#[cfg(windows)]
fn find_pid_for_remote(remote_ip: &str) -> Option<u32> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = format!(
        "(Get-NetTCPConnection -RemoteAddress {} -ErrorAction SilentlyContinue \
         | Where-Object {{ $_.State -eq 'Established' }} \
         | Select-Object -First 1 -ExpandProperty OwningProcess)",
        remote_ip
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<u32>().ok()
}

#[cfg(not(windows))]
fn find_pid_for_remote(_remote_ip: &str) -> Option<u32> {
    None
}

#[cfg(windows)]
pub fn enable_debug_privilege() -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES,
        SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
            .is_err()
        {
            return Err("OpenProcessToken failed".into());
        }

        let mut luid = LUID::default();
        let name: Vec<u16> = "SeDebugPrivilege\0".encode_utf16().collect();
        if LookupPrivilegeValueW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(name.as_ptr()),
            &mut luid,
        )
            .is_err()
        {
            let _ = CloseHandle(token);
            return Err("LookupPrivilegeValueW failed".into());
        }

        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        let res = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        let _ = CloseHandle(token);

        if res.is_err() {
            return Err("AdjustTokenPrivileges failed".into());
        }

        Ok(())
    }
}

#[cfg(not(windows))]
pub fn enable_debug_privilege() -> Result<(), String> {
    Err("SeDebugPrivilege is Windows-only".into())
}