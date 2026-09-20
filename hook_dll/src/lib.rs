use ib_hook::inline::InlineHook;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// ============================================================
// WINDOWS API BINDINGS
// ============================================================

#[cfg(windows)]
mod win {
    pub type Handle = *mut core::ffi::c_void;
    pub type Bool = i32;
    pub type Dword = u32;

    pub const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    pub const PIPE_ACCESS_INBOUND: Dword = 0x00000001;
    pub const PIPE_TYPE_BYTE: Dword = 0x00000000;
    pub const PIPE_WAIT: Dword = 0x00000000;
    pub const BUFFER_SIZE: Dword = 65536;
    pub const GENERIC_WRITE: Dword = 0x40000000;
    pub const OPEN_EXISTING: Dword = 3;
    pub const FILE_ATTRIBUTE_NORMAL: Dword = 0x00000080;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateNamedPipeW(
            lpName: *const u16,
            dwOpenMode: Dword,
            dwPipeMode: Dword,
            nMaxInstances: Dword,
            nOutBufferSize: Dword,
            nInBufferSize: Dword,
            nDefaultTimeOut: Dword,
            lpSecurityAttributes: *mut core::ffi::c_void,
        ) -> Handle;

        pub fn ConnectNamedPipe(hNamedPipe: Handle, lpOverlapped: *mut core::ffi::c_void) -> Bool;

        pub fn DisconnectNamedPipe(hNamedPipe: Handle) -> Bool;

        pub fn ReadFile(
            hFile: Handle,
            lpBuffer: *mut u8,
            nNumberOfBytesToRead: Dword,
            lpNumberOfBytesRead: *mut Dword,
            lpOverlapped: *mut core::ffi::c_void,
        ) -> Bool;

        pub fn WriteFile(
            hFile: Handle,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: Dword,
            lpNumberOfBytesWritten: *mut Dword,
            lpOverlapped: *mut core::ffi::c_void,
        ) -> Bool;

        pub fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: Dword,
            dwShareMode: Dword,
            lpSecurityAttributes: *mut core::ffi::c_void,
            dwCreationDisposition: Dword,
            dwFlagsAndAttributes: Dword,
            hTemplateFile: Handle,
        ) -> Handle;

        pub fn CloseHandle(hObject: Handle) -> Bool;

        pub fn GetModuleHandleW(lpModuleName: *const u16) -> Handle;

        pub fn GetProcAddress(hModule: Handle, lpProcName: *const u8) -> *mut core::ffi::c_void;

        pub fn FreeLibraryAndExitThread(hLibModule: Handle, dwExitCode: Dword) -> !;

        pub fn GetModuleHandleExW(
            dwFlags: Dword,
            lpModuleName: *const u16,
            phModule: *mut Handle,
        ) -> Bool;

        pub fn OutputDebugStringA(lpOutputString: *const u8);
    }

    pub fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

// ============================================================
// CONFIGURATION
// ============================================================

const PACKET_PIPE_NAME: &str = r"\\.\pipe\hook_packets";
const CONTROL_PIPE_NAME: &str = r"\\.\pipe\hook_control";

const DIR_SENT: u8 = 0x01;
const DIR_RECV: u8 = 0x02;

// ============================================================
// FUNCTION SIGNATURES
// ============================================================

type SendFn = unsafe extern "system" fn(usize, *const u8, i32, i32) -> i32;
type RecvFn = unsafe extern "system" fn(usize, *mut u8, i32, i32) -> i32;

static ORIGINAL_SEND: OnceLock<SendFn> = OnceLock::new();
static ORIGINAL_RECV: OnceLock<RecvFn> = OnceLock::new();

static HOOK_SEND: OnceLock<Mutex<Option<InlineHook<SendFn>>>> = OnceLock::new();
static HOOK_RECV: OnceLock<Mutex<Option<InlineHook<RecvFn>>>> = OnceLock::new();

static INTERCEPTING: AtomicBool = AtomicBool::new(true);

// ============================================================
// LOGGING
// ============================================================

fn log_message(msg: &str) {
    #[cfg(windows)]
    unsafe {
        let full = format!("[HOOK_DLL] {}\n", msg);
        let mut bytes = full.into_bytes();
        bytes.push(0);
        win::OutputDebugStringA(bytes.as_ptr());
    }

    #[cfg(not(windows))]
    {
        println!("[HOOK_DLL] {}", msg);
    }
}

// ============================================================
// HOOKED FUNCTIONS
// ============================================================

unsafe extern "system" fn hooked_send(
    socket: usize,
    buf: *const u8,
    len: i32,
    flags: i32,
) -> i32 {
    if INTERCEPTING.load(Ordering::Relaxed) && !buf.is_null() && len > 0 && len < 65536 {
        let slice = std::slice::from_raw_parts(buf, len as usize);
        send_framed(DIR_SENT, slice);
    }

    match ORIGINAL_SEND.get() {
        Some(f) => f(socket, buf, len, flags),
        None => -1,
    }
}

unsafe extern "system" fn hooked_recv(
    socket: usize,
    buf: *mut u8,
    len: i32,
    flags: i32,
) -> i32 {
    let result = match ORIGINAL_RECV.get() {
        Some(f) => f(socket, buf, len, flags),
        None => return -1,
    };

    if result > 0
        && INTERCEPTING.load(Ordering::Relaxed)
        && !buf.is_null()
        && result < 65536
    {
        let slice = std::slice::from_raw_parts(buf, result as usize);
        send_framed(DIR_RECV, slice);
    }

    result
}

// ============================================================
// PACKET PIPE
// ============================================================

fn send_framed(direction: u8, bytes: &[u8]) {
    #[cfg(windows)]
    unsafe {
        let name = win::to_wide(PACKET_PIPE_NAME);
        let handle = win::CreateFileW(
            name.as_ptr(),
            win::GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            win::OPEN_EXISTING,
            win::FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );

        if handle == win::INVALID_HANDLE_VALUE {
            return;
        }

        let len = bytes.len() as u32;
        let mut header = [0u8; 5];
        header[0] = direction;
        header[1..5].copy_from_slice(&len.to_le_bytes());

        let mut written: win::Dword = 0;

        let _ = win::WriteFile(
            handle,
            header.as_ptr(),
            5,
            &mut written,
            std::ptr::null_mut(),
        );
        let _ = win::WriteFile(
            handle,
            bytes.as_ptr(),
            len,
            &mut written,
            std::ptr::null_mut(),
        );

        let _ = win::CloseHandle(handle);
    }
}

// ============================================================
// CONTROL PIPE
// ============================================================

fn spawn_control_thread() {
    #[cfg(windows)]
    std::thread::spawn(|| unsafe {
        let name = win::to_wide(CONTROL_PIPE_NAME);

        let handle = win::CreateNamedPipeW(
            name.as_ptr(),
            win::PIPE_ACCESS_INBOUND,
            win::PIPE_TYPE_BYTE | win::PIPE_WAIT,
            1,
            win::BUFFER_SIZE,
            win::BUFFER_SIZE,
            0,
            std::ptr::null_mut(),
        );

        if handle == win::INVALID_HANDLE_VALUE {
            log_message("failed to create control pipe");
            return;
        }

        log_message("control pipe ready");
        let _ = win::ConnectNamedPipe(handle, std::ptr::null_mut());

        let mut buf = [0u8; 32];
        let mut read: win::Dword = 0;
        let _ = win::ReadFile(
            handle,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        );

        let _ = win::DisconnectNamedPipe(handle);
        let _ = win::CloseHandle(handle);

        if read > 0 {
            let cmd = String::from_utf8_lossy(&buf[..read as usize]);
            if cmd.trim() == "shutdown" {
                log_message("shutdown requested");
                perform_shutdown();
            }
        }
    });
}

fn perform_shutdown() {
    INTERCEPTING.store(false, Ordering::SeqCst);

    if let Some(cell) = HOOK_SEND.get() {
        if let Ok(mut g) = cell.lock() {
            if let Some(h) = g.take() {
                drop(h);
            }
        }
    }
    if let Some(cell) = HOOK_RECV.get() {
        if let Ok(mut g) = cell.lock() {
            if let Some(h) = g.take() {
                drop(h);
            }
        }
    }

    #[cfg(windows)]
    unsafe {
        const FLAG_FROM_ADDRESS: win::Dword = 0x00000004;
        let mut module: win::Handle = std::ptr::null_mut();
        let self_addr = apply_hook as *const () as *const u16;
        if win::GetModuleHandleExW(FLAG_FROM_ADDRESS, self_addr, &mut module) != 0 {
            win::FreeLibraryAndExitThread(module, 0);
        }
    }
}

// ============================================================
// ENTRY POINT
// ============================================================

fn apply_hook() {
    HOOK_SEND.get_or_init(|| Mutex::new(None));
    HOOK_RECV.get_or_init(|| Mutex::new(None));

    log_message("apply_hook invoked");

    #[cfg(windows)]
    unsafe {
        let module_name = win::to_wide("ws2_32.dll");
        let module = win::GetModuleHandleW(module_name.as_ptr());

        if module.is_null() {
            log_message("ws2_32.dll not loaded in this process");
            return;
        }

        let send_addr = win::GetProcAddress(module, b"send\0".as_ptr());
        let recv_addr = win::GetProcAddress(module, b"recv\0".as_ptr());

        if send_addr.is_null() || recv_addr.is_null() {
            log_message("could not resolve send/recv exports");
            return;
        }

        let _ = ORIGINAL_SEND.set(std::mem::transmute(send_addr));
        let _ = ORIGINAL_RECV.set(std::mem::transmute(recv_addr));

        let orig_send = *ORIGINAL_SEND.get().unwrap();
        match InlineHook::new_enabled(orig_send, hooked_send) {
            Ok(h) => {
                if let Some(cell) = HOOK_SEND.get() {
                    if let Ok(mut g) = cell.lock() {
                        *g = Some(h);
                        log_message("send hooked");
                    }
                }
            }
            Err(e) => log_message(&format!("send hook failed: {:?}", e)),
        }

        let orig_recv = *ORIGINAL_RECV.get().unwrap();
        match InlineHook::new_enabled(orig_recv, hooked_recv) {
            Ok(h) => {
                if let Some(cell) = HOOK_RECV.get() {
                    if let Ok(mut g) = cell.lock() {
                        *g = Some(h);
                        log_message("recv hooked");
                    }
                }
            }
            Err(e) => log_message(&format!("recv hook failed: {:?}", e)),
        }
    }

    spawn_control_thread();
}

#[no_mangle]
pub extern "system" fn DllMain(
    _hinst: *mut core::ffi::c_void,
    reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            apply_hook();
        });
    }
    1
}