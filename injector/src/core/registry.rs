#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hkey: *mut core::ffi::c_void,
        lpsubkey: *const u16,
        uloptions: u32,
        samdesired: u32,
        phkresult: *mut *mut core::ffi::c_void,
    ) -> i32;

    fn RegCloseKey(hkey: *mut core::ffi::c_void) -> i32;

    fn RegEnumKeyExW(
        hkey: *mut core::ffi::c_void,
        dwindex: u32,
        lpname: *mut u16,
        lpcchname: *mut u32,
        lpreserved: *mut u32,
        lpclass: *mut u16,
        lpcchclass: *mut u32,
        lpftlastwritetime: *mut core::ffi::c_void,
    ) -> i32;

    fn RegQueryValueExW(
        hkey: *mut core::ffi::c_void,
        lpvaluename: *const u16,
        lpreserved: *mut u32,
        lptype: *mut u32,
        lpdata: *mut u8,
        lpcbdata: *mut u32,
    ) -> i32;
}

const HKEY_LOCAL_MACHINE: isize = 0x80000002u32 as i32 as isize;
const HKEY_CURRENT_USER: isize = 0x80000001u32 as i32 as isize;
const KEY_READ: u32 = 0x20019;
const KEY_WOW64_64KEY: u32 = 0x0100;
const KEY_WOW64_32KEY: u32 = 0x0200;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const ERROR_SUCCESS: i32 = 0;
const ERROR_NO_MORE_ITEMS: i32 = 259;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn query_string(hkey: *mut core::ffi::c_void, name: &str) -> Option<String> {
    let name_w = wide(name);
    let mut ty: u32 = 0;
    let mut size: u32 = 0;

    let r = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if r != ERROR_SUCCESS || size == 0 {
        return None;
    }
    if ty != REG_SZ && ty != REG_EXPAND_SZ {
        return None;
    }
    if size > 65536 {
        return None;
    }

    let mut buf: Vec<u8> = vec![0; size as usize];
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut size,
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }

    while buf.len() >= 2 && buf[buf.len() - 1] == 0 && buf[buf.len() - 2] == 0 {
        buf.truncate(buf.len() - 2);
    }
    if buf.len() < 2 {
        return None;
    }

    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(OsString::from_wide(&u16s).to_string_lossy().to_string())
}

fn strip_icon_suffix(s: &str) -> String {
    if let Some(idx) = s.rfind(',') {
        let after = &s[idx + 1..];
        if after
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == ' ')
        {
            return s[..idx].to_string();
        }
    }
    s.to_string()
}

fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn is_guid_name(name: &str) -> bool {
    let trimmed = name.trim();
    let candidate = trimmed.trim_matches(|c| c == '{' || c == '}');
    let parts: Vec<&str> = candidate.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
        .iter()
        .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_version_like_name(name: &str) -> bool {
    let trimmed = name.trim();
    let lower = trimmed.to_lowercase();

    // Always accept these known games, even if the registered name
    // contains digits or version-like text.
    if lower.contains("artix")
        || lower.contains("aqw")
        || lower.contains("adventure quest")
    {
        return false;
    }

    // Reject braces-wrapped GUIDs.
    if is_guid_name(trimmed) {
        return true;
    }

    if trimmed.len() < 3 {
        return true;
    }

    // Pure version strings like "1.50.1" or "153.0.4234.48".
    let only_version_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_' || c == '+');
    if only_version_chars {
        return true;
    }

    // Mostly digits and dots, like "25.206.1021.0003".
    let digits = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    let dots = trimmed.chars().filter(|c| *c == '.').count();
    if dots >= 2 && digits * 2 >= trimmed.len() {
        return true;
    }

    // Names that are mostly uppercase hex — GUIDs without braces.
    let compact_hex: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if compact_hex.len() == trimmed.len()
        && compact_hex.len() >= 16
        && trimmed.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }

    // Known non-app patterns.
    const SUSPECT: &[&str] = &[
        "update for",
        "security update",
        "hotfix for",
        "64bit",
        "32bit",
        "redistributable",
        "microsoft visual c++",
        "microsoft .net",
        "microsoft edge update",
        "windows sdk",
    ];
    if SUSPECT.iter().any(|k| lower.contains(k)) {
        return true;
    }

    false
}

fn pick_main_exe_in(folder: &std::path::Path, display_name: &str) -> Option<String> {
    let entries = std::fs::read_dir(folder).ok()?;

    const REJECT: &[&str] = &[
        "uninstall", "unins", "setup", "installer",
        "unins000", "unins001",
        "crash", "report", "updater", "update",
        "helper", "service", "daemon",
    ];

    let name_lower = display_name.to_lowercase();
    let primary_token = name_lower
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();

    struct Candidate {
        path: String,
        score: u32,
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if ext != "exe" {
            continue;
        }

        let exe_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let exe_lower = exe_name.to_lowercase();

        if REJECT.iter().any(|k| exe_lower.contains(k)) {
            continue;
        }

        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size < 8_000 {
            continue;
        }

        let mut score: u32 = 0;
        if exe_lower == name_lower {
            score += 200;
        } else if !primary_token.is_empty()
            && (exe_lower.contains(&primary_token) || primary_token.contains(&exe_lower))
        {
            score += 100;
        } else if name_lower
            .split_whitespace()
            .any(|w| w.len() >= 3 && exe_lower.contains(w))
        {
            score += 50;
        }

        score += (size / 1_000_000).min(30) as u32;

        candidates.push(Candidate {
            path: path.to_string_lossy().to_string(),
            score,
        });
    }

    candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
    candidates.into_iter().next().map(|c| c.path)
}

pub fn enumerate_app_paths() -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();

    let roots: [(isize, &str); 2] = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
        ),
    ];

    for (root, subkey) in roots {
        for wow in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let mut hkey: *mut core::ffi::c_void = std::ptr::null_mut();
            let sub_w = wide(subkey);
            let r = unsafe {
                RegOpenKeyExW(
                    root as *mut _,
                    sub_w.as_ptr(),
                    0,
                    KEY_READ | wow,
                    &mut hkey,
                )
            };
            if r != ERROR_SUCCESS || hkey.is_null() {
                continue;
            }

            let mut idx: u32 = 0;
            loop {
                let mut name_buf = [0u16; 512];
                let mut name_len: u32 = 512;
                let r = unsafe {
                    RegEnumKeyExW(
                        hkey,
                        idx,
                        name_buf.as_mut_ptr(),
                        &mut name_len,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if r == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if r != ERROR_SUCCESS {
                    idx += 1;
                    continue;
                }

                let sub_name = OsString::from_wide(&name_buf[..name_len as usize])
                    .to_string_lossy()
                    .to_string();

                let mut sub_hkey: *mut core::ffi::c_void = std::ptr::null_mut();
                let sub_w = wide(&sub_name);
                let r2 =
                    unsafe { RegOpenKeyExW(hkey, sub_w.as_ptr(), 0, KEY_READ, &mut sub_hkey) };
                if r2 == ERROR_SUCCESS && !sub_hkey.is_null() {
                    if let Some(path) = query_string(sub_hkey, "") {
                        let path = strip_quotes(&path);
                        let display = sub_name
                            .trim_end_matches(".exe")
                            .trim_end_matches(".EXE")
                            .to_string();
                        out.push((display, path));
                    }
                    unsafe {
                        RegCloseKey(sub_hkey);
                    }
                }

                idx += 1;
            }

            unsafe {
                RegCloseKey(hkey);
            }
        }
    }

    Ok(out)
}

pub fn enumerate_uninstall_entries() -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();

    let roots: [(isize, &str); 4] = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];

    for (root, subkey) in roots {
        let mut hkey: *mut core::ffi::c_void = std::ptr::null_mut();
        let sub_w = wide(subkey);
        let r = unsafe {
            RegOpenKeyExW(
                root as *mut _,
                sub_w.as_ptr(),
                0,
                KEY_READ,
                &mut hkey,
            )
        };
        if r != ERROR_SUCCESS || hkey.is_null() {
            continue;
        }

        let mut idx: u32 = 0;
        loop {
            let mut name_buf = [0u16; 512];
            let mut name_len: u32 = 512;
            let r = unsafe {
                RegEnumKeyExW(
                    hkey,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            if r != ERROR_SUCCESS {
                idx += 1;
                continue;
            }

            let sub_name = OsString::from_wide(&name_buf[..name_len as usize])
                .to_string_lossy()
                .to_string();

            let mut sub_hkey: *mut core::ffi::c_void = std::ptr::null_mut();
            let sub_w = wide(&sub_name);
            let r2 = unsafe {
                RegOpenKeyExW(hkey, sub_w.as_ptr(), 0, KEY_READ, &mut sub_hkey)
            };
            if r2 == ERROR_SUCCESS && !sub_hkey.is_null() {
                if let Some(sys) = query_string(sub_hkey, "SystemComponent") {
                    if sys == "1" {
                        unsafe { RegCloseKey(sub_hkey); }
                        idx += 1;
                        continue;
                    }
                }

                let display_name = match query_string(sub_hkey, "DisplayName") {
                    Some(n) if !is_version_like_name(&n) => n,
                    _ => {
                        unsafe { RegCloseKey(sub_hkey); }
                        idx += 1;
                        continue;
                    }
                };

                let mut exe: Option<String> = None;

                if let Some(icon) = query_string(sub_hkey, "DisplayIcon") {
                    let cleaned = strip_icon_suffix(&strip_quotes(&icon));
                    if cleaned.to_lowercase().ends_with(".exe") {
                        exe = Some(cleaned);
                    }
                }

                if exe.is_none() {
                    if let Some(loc) = query_string(sub_hkey, "InstallLocation") {
                        let loc = strip_quotes(&loc);
                        let guess = std::path::Path::new(&loc)
                            .join(format!("{}.exe", display_name));
                        if guess.exists() {
                            exe = Some(guess.to_string_lossy().to_string());
                        }
                    }
                }

                if exe.is_none() {
                    if let Some(uninst) = query_string(sub_hkey, "UninstallString")
                        .or_else(|| query_string(sub_hkey, "QuietUninstallString"))
                    {
                        let cleaned = strip_quotes(&uninst);
                        let path_only = if cleaned.starts_with('"') {
                            cleaned
                                .trim_start_matches('"')
                                .split('"')
                                .next()
                                .unwrap_or("")
                                .to_string()
                        } else {
                            cleaned
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .to_string()
                        };

                        if let Some(folder) = std::path::Path::new(&path_only).parent() {
                            if let Some(found) = pick_main_exe_in(folder, &display_name) {
                                exe = Some(found);
                            }
                        }
                    }
                }

                if let Some(path) = exe {
                    if !path.is_empty() {
                        out.push((display_name, path));
                    }
                }

                unsafe {
                    RegCloseKey(sub_hkey);
                }
            }

            idx += 1;
        }

        unsafe {
            RegCloseKey(hkey);
        }
    }

    Ok(out)
}

#[cfg(not(windows))]
pub fn enumerate_app_paths() -> Result<Vec<(String, String)>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
pub fn enumerate_uninstall_entries() -> Result<Vec<(String, String)>, String> {
    Ok(Vec::new())
}