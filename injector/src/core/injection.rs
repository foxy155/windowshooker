//! Raw FFI DLL injection. No external crates.
//!
//! Supports two methods:
//!   - CreateRemoteThread + LoadLibraryW
//!   - QueueUserAPC + LoadLibraryW
//!
//! Both load the target DLL into the remote process by calling
//! LoadLibraryW on a UTF-16 path we write into its memory.

#![cfg(windows)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

// ---------- Kernel32 / Win32 FFI declarations ----------

type Handle = *mut c_void;
type Bool = i32;
type Dword = u32;
type Lpv = *mut c_void;
type Lpcv = *const c_void;

const PROCESS_ALL_ACCESS: Dword = 0x1F0FFF;
const MEM_COMMIT: Dword = 0x1000;
const MEM_RESERVE: Dword = 0x2000;
const MEM_RELEASE: Dword = 0x8000;
const PAGE_READWRITE: Dword = 0x04;
const INFINITE: Dword = 0xFFFFFFFF;
const TH32CS_SNAPTHREAD: Dword = 0x00000004;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const THREAD_SET_CONTEXT: Dword = 0x0010;

#[repr(C)]
struct ThreadEntry32W {
    dw_size: Dword,
    cnt_usage: Dword,
    th32_thread_id: Dword,
    th32_owner_process_id: Dword,
    tp_base_pri: i32,
    dw_flags: Dword,
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(
        dwDesiredAccess: Dword,
        bInheritHandle: Bool,
        dwProcessId: Dword,
    ) -> Handle;

    fn CloseHandle(hObject: Handle) -> Bool;

    fn VirtualAllocEx(
        hProcess: Handle,
        lpAddress: Lpv,
        dwSize: usize,
        flAllocationType: Dword,
        flProtect: Dword,
    ) -> Lpv;

    fn VirtualFreeEx(
        hProcess: Handle,
        lpAddress: Lpv,
        dwSize: usize,
        dwFreeType: Dword,
    ) -> Bool;

    fn WriteProcessMemory(
        hProcess: Handle,
        lpBaseAddress: Lpv,
        lpBuffer: Lpcv,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> Bool;

    fn GetModuleHandleW(lpModuleName: *const u16) -> Handle;

    fn GetProcAddress(hModule: Handle, lpProcName: *const u8) -> Lpv;

    fn CreateRemoteThread(
        hProcess: Handle,
        lpThreadAttributes: Lpv,
        dwStackSize: usize,
        lpStartAddress: extern "system" fn(Lpv) -> Dword,
        lpParameter: Lpv,
        dwCreationFlags: Dword,
        lpThreadId: *mut Dword,
    ) -> Handle;

    fn WaitForSingleObject(hHandle: Handle, dwMilliseconds: Dword) -> Dword;

    fn CreateToolhelp32Snapshot(dwFlags: Dword, th32ProcessID: Dword) -> Handle;

    fn Thread32First(hSnapshot: Handle, lpte: *mut ThreadEntry32W) -> Bool;

    fn Thread32Next(hSnapshot: Handle, lpte: *mut ThreadEntry32W) -> Bool;

    fn OpenThread(
        dwDesiredAccess: Dword,
        bInheritHandle: Bool,
        dwThreadId: Dword,
    ) -> Handle;

    fn QueueUserAPC(
        pfnAPC: extern "system" fn(usize),
        hThread: Handle,
        dwData: usize,
    ) -> Dword;

    fn GetLastError() -> Dword;
}

// ---------- Public API ----------

#[derive(Clone, Copy, PartialEq)]
pub enum InjectionMethod {
    CreateRemoteThread,
    QueueUserAPC,
}

impl InjectionMethod {
    pub fn label(&self) -> &'static str {
        match self {
            InjectionMethod::CreateRemoteThread => "CreateRemoteThread",
            InjectionMethod::QueueUserAPC => "QueueUserAPC",
        }
    }
}

/// Load a DLL into a remote process by PID.
pub fn inject(pid: u32, dll_path: &str, method: InjectionMethod) -> Result<(), String> {
    if dll_path.is_empty() {
        return Err("dll path is empty".into());
    }

    let path_wide: Vec<u16> = std::ffi::OsStr::new(dll_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_bytes = path_wide.len() * 2;

    unsafe {
        let process = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if process.is_null() {
            return Err(format!(
                "OpenProcess failed for PID {} (GetLastError={})",
                pid,
                GetLastError()
            ));
        }

        let result = (|| -> Result<(), String> {
            let remote_path = VirtualAllocEx(
                process,
                std::ptr::null_mut(),
                path_bytes,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_path.is_null() {
                return Err(format!(
                    "VirtualAllocEx failed (GetLastError={})",
                    GetLastError()
                ));
            }

            let mut written: usize = 0;
            let ok = WriteProcessMemory(
                process,
                remote_path,
                path_wide.as_ptr() as Lpcv,
                path_bytes,
                &mut written,
            );
            if ok == 0 {
                let _ = VirtualFreeEx(process, remote_path, 0, MEM_RELEASE);
                return Err(format!(
                    "WriteProcessMemory failed (GetLastError={})",
                    GetLastError()
                ));
            }

            let kernel32 = GetModuleHandleW(wide("kernel32.dll").as_ptr());
            if kernel32.is_null() {
                return Err("GetModuleHandleW(kernel32.dll) failed".into());
            }
            let load_library = GetProcAddress(kernel32, b"LoadLibraryW\0".as_ptr());
            if load_library.is_null() {
                return Err("GetProcAddress(LoadLibraryW) failed".into());
            }
            let load_library_fn: extern "system" fn(Lpv) -> Dword =
                std::mem::transmute(load_library);

            match method {
                InjectionMethod::CreateRemoteThread => {
                    let thread = CreateRemoteThread(
                        process,
                        std::ptr::null_mut(),
                        0,
                        load_library_fn,
                        remote_path,
                        0,
                        std::ptr::null_mut(),
                    );
                    if thread.is_null() {
                        let _ = VirtualFreeEx(process, remote_path, 0, MEM_RELEASE);
                        return Err(format!(
                            "CreateRemoteThread failed (GetLastError={})",
                            GetLastError()
                        ));
                    }
                    WaitForSingleObject(thread, INFINITE);
                    let _ = CloseHandle(thread);
                }
                InjectionMethod::QueueUserAPC => {
                    let queued = queue_loadlibrary_on_all_threads(
                        pid,
                        load_library,
                        remote_path as usize,
                    )?;
                    if queued == 0 {
                        let _ = VirtualFreeEx(process, remote_path, 0, MEM_RELEASE);
                        return Err(
                            "QueueUserAPC: no threads accepted the APC".into(),
                        );
                    }
                }
            }

            Ok(())
        })();

        let _ = CloseHandle(process);
        result
    }
}

// ---------- Helpers ----------

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

unsafe fn queue_loadlibrary_on_all_threads(
    pid: u32,
    load_library_addr: Lpv,
    remote_path: usize,
) -> Result<u32, String> {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "CreateToolhelp32Snapshot failed (GetLastError={})",
            GetLastError()
        ));
    }

    // The APC function pointer. We hand QueueUserAPC the raw address
    // of LoadLibraryW; the OS interprets it as an APC routine whose
    // single argument is our remote_path. LoadLibraryW's signature
    // is (LPCWSTR) -> HMODULE, so the types line up when LoadLibraryW
    // is used as an APC callback.
    let apc_fn: extern "system" fn(usize) = std::mem::transmute(load_library_addr);

    let mut entry: ThreadEntry32W = std::mem::zeroed();
    entry.dw_size = std::mem::size_of::<ThreadEntry32W>() as Dword;

    let mut queued: u32 = 0;

    if Thread32First(snapshot, &mut entry) != 0 {
        loop {
            if entry.th32_owner_process_id == pid {
                let thread = OpenThread(THREAD_SET_CONTEXT, 0, entry.th32_thread_id);
                if !thread.is_null() {
                    let ok = QueueUserAPC(apc_fn, thread, remote_path);
                    if ok != 0 {
                        queued += 1;
                    }
                    let _ = CloseHandle(thread);
                }
            }
            if Thread32Next(snapshot, &mut entry) == 0 {
                break;
            }
        }
    }

    let _ = CloseHandle(snapshot);
    Ok(queued)
}