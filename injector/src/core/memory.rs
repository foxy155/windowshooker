//! Win32 memory access for a remote process.
//!
//! Everything here is a safe-ish wrapper around ReadProcessMemory /
//! WriteProcessMemory / VirtualQueryEx. The unsafe blocks are confined
//! to the raw FFI calls; the public API is fallible but not unsafe.
//!
//! Windows-only. On other platforms this module compiles to stubs that
//! return errors, so the crate still builds on CI.

#![cfg(windows)]

use std::ffi::c_void;

// ============================================================
// RAW FFI
// ============================================================

type Handle = *mut c_void;
type Bool = i32;
type Dword = u32;
type SizeT = usize;

const PROCESS_ALL_ACCESS: Dword = 0x1F0FFF;
const PROCESS_VM_READ: Dword = 0x0010;
const PROCESS_VM_WRITE: Dword = 0x0020;
const PROCESS_VM_OPERATION: Dword = 0x0008;
const PROCESS_QUERY_INFORMATION: Dword = 0x0400;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;

const MEM_COMMIT: Dword = 0x1000;
const MEM_PRIVATE: Dword = 0x20000;
const MEM_MAPPED: Dword = 0x40000;
const MEM_IMAGE: Dword = 0x1000000;

const PAGE_NOACCESS: Dword = 0x01;
const PAGE_READONLY: Dword = 0x02;
const PAGE_READWRITE: Dword = 0x04;
const PAGE_WRITECOPY: Dword = 0x08;
const PAGE_EXECUTE: Dword = 0x10;
const PAGE_EXECUTE_READ: Dword = 0x20;
const PAGE_EXECUTE_READWRITE: Dword = 0x40;
const PAGE_EXECUTE_WRITECOPY: Dword = 0x80;
const PAGE_GUARD: Dword = 0x100;
const PAGE_NOCACHE: Dword = 0x200;

#[repr(C)]
struct MemoryBasicInformation {
    base_address: *mut c_void,
    allocation_base: *mut c_void,
    allocation_protect: Dword,
    partition_id: u16,
    _pad1: u16,
    region_size: SizeT,
    state: Dword,
    protect: Dword,
    ty: Dword,
    _pad2: Dword,
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(
        dwDesiredAccess: Dword,
        bInheritHandle: Bool,
        dwProcessId: Dword,
    ) -> Handle;

    fn CloseHandle(hObject: Handle) -> Bool;

    fn ReadProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *const c_void,
        lpBuffer: *mut c_void,
        nSize: SizeT,
        lpNumberOfBytesRead: *mut SizeT,
    ) -> Bool;

    fn WriteProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *mut c_void,
        lpBuffer: *const c_void,
        nSize: SizeT,
        lpNumberOfBytesWritten: *mut SizeT,
    ) -> Bool;

    fn VirtualQueryEx(
        hProcess: Handle,
        lpAddress: *const c_void,
        lpBuffer: *mut MemoryBasicInformation,
        dwLength: SizeT,
    ) -> SizeT;

    fn GetLastError() -> Dword;
}

// ============================================================
// PUBLIC TYPES
// ============================================================

/// Handle to a target process with memory access rights.
pub struct MemoryReader {
    handle: Handle,
    pub pid: u32,
}

// The handle is just an OS handle, safe to move between threads as
// long as we don't use it concurrently. The Mutex in App ensures that.
unsafe impl Send for MemoryReader {}

impl MemoryReader {
    /// Open a process with VM read/write/query rights.
    pub fn open(pid: u32) -> Result<Self, String> {
        let access = PROCESS_VM_READ
            | PROCESS_VM_WRITE
            | PROCESS_VM_OPERATION
            | PROCESS_QUERY_INFORMATION;

        unsafe {
            let handle = OpenProcess(access, 0, pid);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "OpenProcess failed for PID {} (GetLastError={})",
                    pid,
                    GetLastError()
                ));
            }
            Ok(Self { handle, pid })
        }
    }

    /// Read a block of bytes at `address`.
    pub fn read(&self, address: u64, len: usize) -> Result<Vec<u8>, String> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; len];
        let mut read: SizeT = 0;
        unsafe {
            let ok = ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                len,
                &mut read,
            );
            if ok == 0 {
                return Err(format!(
                    "ReadProcessMemory failed at {:#x} (GetLastError={})",
                    address,
                    GetLastError()
                ));
            }
        }
        buf.truncate(read);
        Ok(buf)
    }

    /// Write a block of bytes at `address`.
    pub fn write(&self, address: u64, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut written: SizeT = 0;
        unsafe {
            let ok = WriteProcessMemory(
                self.handle,
                address as *mut c_void,
                bytes.as_ptr() as *const c_void,
                bytes.len(),
                &mut written,
            );
            if ok == 0 || written != bytes.len() {
                return Err(format!(
                    "WriteProcessMemory wrote {}/{} bytes at {:#x} (GetLastError={})",
                    written,
                    bytes.len(),
                    address,
                    GetLastError()
                ));
            }
        }
        Ok(())
    }

    /// Query the region that contains `address`.
    pub fn query_region(&self, address: u64) -> Option<MemoryRegion> {
        let mut mbi: MemoryBasicInformation = unsafe { std::mem::zeroed() };
        unsafe {
            let ret = VirtualQueryEx(
                self.handle,
                address as *const c_void,
                &mut mbi,
                std::mem::size_of::<MemoryBasicInformation>(),
            );
            if ret == 0 {
                return None;
            }
        }
        Some(MemoryRegion {
            base: mbi.base_address as u64,
            size: mbi.region_size,
            state: mbi.state,
            protect: mbi.protect,
            ty: mbi.ty,
        })
    }

    /// Enumerate every committed region in the process address space.
    /// Walks the address space via VirtualQueryEx starting at 0.
    pub fn regions(&self) -> Vec<MemoryRegion> {
        let mut out = Vec::new();
        let mut addr: u64 = 0;

        // Avoid walking into the very top of the user address space on
        // 64-bit Windows where nothing lives.
        const MAX_ADDR: u64 = 0x0000_7FFF_FFFF_FFFF;

        while addr < MAX_ADDR {
            let Some(region) = self.query_region(addr) else {
                break;
            };
            if region.state & MEM_COMMIT != 0 {
                out.push(region.clone());
            }
            let next = region.base.saturating_add(region.size as u64);
            if next <= addr {
                break; // paranoia against zero-size regions
            }
            addr = next;
        }

        out
    }
}

impl Drop for MemoryReader {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

// ============================================================
// MEMORY REGION
// ============================================================

#[derive(Clone)]
pub struct MemoryRegion {
    pub base: u64,
    pub size: usize,
    pub state: u32,
    pub protect: u32,
    pub ty: u32,
}

impl MemoryRegion {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.size as u64)
    }

    pub fn is_readable(&self) -> bool {
        const READABLE: u32 = PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY;
        self.protect & READABLE != 0 && self.protect & PAGE_GUARD == 0
    }

    pub fn is_writable(&self) -> bool {
        const WRITABLE: u32 = PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY;
        self.protect & WRITABLE != 0 && self.protect & PAGE_GUARD == 0
    }

    pub fn is_executable(&self) -> bool {
        const EXECUTABLE: u32 =
            PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
        self.protect & EXECUTABLE != 0
    }

    pub fn protection_label(&self) -> String {
        let mut parts = Vec::new();
        if self.protect & PAGE_GUARD != 0 {
            parts.push("GUARD");
        }
        if self.protect & PAGE_NOCACHE != 0 {
            parts.push("NOCACHE");
        }

        let base = match self.protect & 0xFF {
            PAGE_NOACCESS => "NOACCESS",
            PAGE_READONLY => "R--",
            PAGE_READWRITE => "RW-",
            PAGE_WRITECOPY => "RWC",
            PAGE_EXECUTE => "--X",
            PAGE_EXECUTE_READ => "R-X",
            PAGE_EXECUTE_READWRITE => "RWX",
            PAGE_EXECUTE_WRITECOPY => "RWCX",
            _ => "???",
        };
        parts.push(base);
        parts.join(" ")
    }

    pub fn type_label(&self) -> &'static str {
        if self.ty & MEM_IMAGE != 0 {
            "image"
        } else if self.ty & MEM_MAPPED != 0 {
            "mapped"
        } else if self.ty & MEM_PRIVATE != 0 {
            "private"
        } else {
            "unknown"
        }
    }
}

// ============================================================
// SCAN — value reading / writing helpers
// ============================================================

/// Types the scanner can search for and read/write.
///
/// `All` is a meta-type: it doesn't represent a single memory layout,
/// but tells the scanner to try every concrete type and merge the
/// results. Each hit records which concrete type it matched.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    All,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float,
    Double,
    Bytes,
    String,
}

impl ValueType {
    pub fn label(&self) -> &'static str {
        match self {
            ValueType::All => "all",
            ValueType::Int8 => "int8",
            ValueType::Int16 => "int16",
            ValueType::Int32 => "int32",
            ValueType::Int64 => "int64",
            ValueType::Uint8 => "uint8",
            ValueType::Uint16 => "uint16",
            ValueType::Uint32 => "uint32",
            ValueType::Uint64 => "uint64",
            ValueType::Float => "float",
            ValueType::Double => "double",
            ValueType::Bytes => "bytes",
            ValueType::String => "string",
        }
    }

    /// Every concrete (non-All) numeric type, in the order we want
    /// them tried when the user selects "all".
    pub fn concrete_types() -> &'static [ValueType] {
        &[
            ValueType::Int32,
            ValueType::Int64,
            ValueType::Float,
            ValueType::Double,
            ValueType::Int16,
            ValueType::Uint16,
            ValueType::Int8,
            ValueType::Uint8,
        ]
    }

    pub fn all() -> [ValueType; 13] {
        [
            ValueType::All,
            ValueType::Int8,
            ValueType::Int16,
            ValueType::Int32,
            ValueType::Int64,
            ValueType::Uint8,
            ValueType::Uint16,
            ValueType::Uint32,
            ValueType::Uint64,
            ValueType::Float,
            ValueType::Double,
            ValueType::Bytes,
            ValueType::String,
        ]
    }

    /// Byte width when this type is a fixed-size scalar. Returns 0
    /// for `All`, `Bytes`, and `String` (which are variadic).
    pub fn byte_size(&self) -> usize {
        match self {
            ValueType::Int8 | ValueType::Uint8 => 1,
            ValueType::Int16 | ValueType::Uint16 => 2,
            ValueType::Int32 | ValueType::Uint32 | ValueType::Float => 4,
            ValueType::Int64 | ValueType::Uint64 | ValueType::Double => 8,
            _ => 0,
        }
    }
}

/// Direction a value moved between the last two reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChangeDir {
    /// Never read twice yet.
    Unknown,
    /// Same value on both reads.
    Same,
    /// Value increased.
    Up,
    /// Value decreased.
    Down,
}

/// A candidate hit from a scan.
#[derive(Clone)]
pub struct ScanHit {
    pub address: u64,
    pub value: String,
    pub previous: Option<String>,
    pub change: ChangeDir,
    pub frozen: bool,
    /// Concrete type this hit was recorded as. Under `ValueType::All`
    /// each hit carries its own concrete type so it can be formatted
    /// correctly on its own.
    pub kind: ValueType,
}

impl ScanHit {
    /// Small helper for constructing a fresh hit with no history.
    pub fn new(address: u64, value: String, kind: ValueType) -> Self {
        Self {
            address,
            value,
            previous: None,
            change: ChangeDir::Unknown,
            frozen: false,
            kind,
        }
    }
}

/// Parse a user-typed value into raw bytes for the given value type.
/// Used to seed the first scan, or to write a new value from the UI.
pub fn parse_value(ty: ValueType, input: &str) -> Result<Vec<u8>, String> {
    let s = input.trim();
    match ty {
        ValueType::All => {
            // In "all" mode we accept any number the user types and
            // reinterpret it as every concrete type in `scan_typed`.
            // Try int64, then f64, then fall back to raw bytes.
            if let Ok(v) = s.parse::<i64>() {
                return Ok(v.to_le_bytes().to_vec());
            }
            if let Ok(v) = s.parse::<f64>() {
                return Ok(v.to_le_bytes().to_vec());
            }
            // Fall back to string bytes so we can still scan for text.
            Ok(s.as_bytes().to_vec())
        },
        ValueType::Int8 => {
            let v: i8 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid int8", s))?;
            Ok(vec![v as u8])
        }
        ValueType::Int16 => {
            let v: i16 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid int16", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Int32 => {
            let v: i32 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid int32", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Int64 => {
            let v: i64 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid int64", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Uint8 => {
            let v: u8 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid uint8", s))?;
            Ok(vec![v])
        }
        ValueType::Uint16 => {
            let v: u16 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid uint16", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Uint32 => {
            let v: u32 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid uint32", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Uint64 => {
            let v: u64 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid uint64", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Float => {
            let v: f32 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid float", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Double => {
            let v: f64 = s
                .parse()
                .map_err(|_| format!("'{}' is not a valid double", s))?;
            Ok(v.to_le_bytes().to_vec())
        }
        ValueType::Bytes => {
            // Accept "AA BB CC" or "aabbcc".
            let cleaned: String = s
                .chars()
                .filter(|c| !c.is_whitespace() && *c != ':')
                .collect();
            if cleaned.len() % 2 != 0 {
                return Err("bytes value must have an even number of hex digits".into());
            }
            let mut out = Vec::with_capacity(cleaned.len() / 2);
            let bytes = cleaned.as_bytes();
            let mut i = 0;
            while i + 1 < bytes.len() {
                let hi = (bytes[i] as char)
                    .to_digit(16)
                    .ok_or_else(|| "bytes value is not valid hex".to_string())?;
                let lo = (bytes[i + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| "bytes value is not valid hex".to_string())?;
                out.push(((hi << 4) | lo) as u8);
                i += 2;
            }
            Ok(out)
        }
        ValueType::String => Ok(s.as_bytes().to_vec()),
    }
}

/// Render raw bytes as a human-readable value for the given type.
pub fn format_value(ty: ValueType, bytes: &[u8]) -> String {
    match ty {
        ValueType::All => hex_preview(bytes),
        ValueType::Int8 => {
            if !bytes.is_empty() {
                format!("{}", bytes[0] as i8)
            } else {
                "?".into()
            }
        }
        ValueType::Int16 => {
            if bytes.len() >= 2 {
                format!("{}", i16::from_le_bytes([bytes[0], bytes[1]]))
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Int32 => {
            if bytes.len() >= 4 {
                let v = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                format!("{}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Int64 => {
            if bytes.len() >= 8 {
                let v = i64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                format!("{}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Uint8 => {
            if !bytes.is_empty() {
                format!("{}", bytes[0])
            } else {
                "?".into()
            }
        }
        ValueType::Uint16 => {
            if bytes.len() >= 2 {
                format!("{}", u16::from_le_bytes([bytes[0], bytes[1]]))
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Uint32 => {
            if bytes.len() >= 4 {
                let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                format!("{}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Uint64 => {
            if bytes.len() >= 8 {
                let v = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                format!("{}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Float => {
            if bytes.len() >= 4 {
                let v = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                format!("{:.4}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Double => {
            if bytes.len() >= 8 {
                let v = f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                format!("{:.6}", v)
            } else {
                hex_preview(bytes)
            }
        }
        ValueType::Bytes => hex_preview(bytes),
        ValueType::String => {
            let s: String = bytes
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                .collect();
            s
        }
    }
}

fn hex_preview(bytes: &[u8]) -> String {
    let take = bytes.len().min(16);
    bytes[..take]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scan every committed readable region for a byte pattern.
/// Returns a bounded number of hits (default 10_000) to keep the UI
/// responsive. The caller decides what to do with them.
pub fn scan_all_regions(
    reader: &MemoryReader,
    needle: &[u8],
    max_hits: usize,
) -> Vec<ScanHit> {
    let mut hits: Vec<ScanHit> = Vec::new();
    if needle.is_empty() {
        return hits;
    }

    const CHUNK: usize = 4 * 1024 * 1024; // 4 MB per read

    for region in reader.regions() {
        if !region.is_readable() || region.size == 0 {
            continue;
        }

        let mut offset: u64 = 0;
        let region_size = region.size as u64;

        while offset < region_size {
            let remaining = region_size - offset;
            let take = remaining.min(CHUNK as u64) as usize;
            let addr = region.base + offset;

            if let Ok(buf) = reader.read(addr, take) {
                // Simple substring search. Fast enough for v1.
                if buf.len() >= needle.len() {
                    let mut i = 0;
                    while i + needle.len() <= buf.len() {
                        if &buf[i..i + needle.len()] == needle {
                            let hit_addr = addr + i as u64;
                            hits.push(ScanHit::new(
                                hit_addr,
                                format_value(
                                    ValueType::Bytes,
                                    &buf[i..i + needle.len()],
                                ),
                                ValueType::Bytes,
                            ));
                            if hits.len() >= max_hits {
                                return hits;
                            }
                        }
                        i += 1;
                    }
                }
            }

            offset = offset.saturating_add(take as u64);
        }
    }

    hits
}

/// Scan for a needle of a specific type. For `ValueType::All`, we run
/// the scan for every concrete type in turn and merge the results,
/// tagging each hit with its concrete type.
pub fn scan_typed(
    reader: &MemoryReader,
    needle: &[u8],
    kind: ValueType,
    max_hits: usize,
) -> Vec<ScanHit> {
    match kind {
        ValueType::All => {
            let mut out = Vec::new();
            let per_type = (max_hits / ValueType::concrete_types().len().max(1)).max(64);

            // Reinterpret the raw input for each concrete type. The
            // caller passed us a byte needle that was parsed as
            // int64 (or f64 as fallback). Recover the numeric value
            // from those bytes and re-encode per type.
            let as_int: Option<i64> = if needle.len() == 8 {
                Some(i64::from_le_bytes([
                    needle[0], needle[1], needle[2], needle[3],
                    needle[4], needle[5], needle[6], needle[7],
                ]))
            } else {
                None
            };
            let as_float: Option<f64> = if needle.len() == 8 {
                Some(f64::from_le_bytes([
                    needle[0], needle[1], needle[2], needle[3],
                    needle[4], needle[5], needle[6], needle[7],
                ]))
            } else {
                None
            };
            let as_text = String::from_utf8_lossy(needle).to_string();

            for t in ValueType::concrete_types() {
                if out.len() >= max_hits {
                    break;
                }
                let bytes_for_type = match *t {
                    ValueType::Int32 => as_int.map(|n| (n as i32).to_le_bytes().to_vec()),
                    ValueType::Int64 => as_int.map(|n| n.to_le_bytes().to_vec()),
                    ValueType::Int16 => as_int.map(|n| (n as i16).to_le_bytes().to_vec()),
                    ValueType::Int8 => as_int.map(|n| vec![n as u8]),
                    ValueType::Uint8 => as_int.map(|n| vec![n as u8]),
                    ValueType::Uint16 => as_int.map(|n| (n as u16).to_le_bytes().to_vec()),
                    ValueType::Uint32 => as_int.map(|n| (n as u32).to_le_bytes().to_vec()),
                    ValueType::Uint64 => as_int.map(|n| (n as u64).to_le_bytes().to_vec()),
                    ValueType::Float => as_float.map(|f| (f as f32).to_le_bytes().to_vec()),
                    ValueType::Double => as_float.map(|f| f.to_le_bytes().to_vec()),
                    _ => None,
                };
                let Some(bytes) = bytes_for_type else { continue };
                let remaining = max_hits.saturating_sub(out.len());
                let budget = per_type.min(remaining);
                let sub = scan_all_regions(reader, &bytes, budget);
                for mut h in sub {
                    h.kind = *t;
                    h.value = format_value(*t, &bytes);
                    out.push(h);
                }
            }

            // Also try the literal text form as a bytes pattern.
            if out.len() < max_hits && !as_text.is_empty() {
                let text_bytes = as_text.as_bytes().to_vec();
                let sub = scan_all_regions(
                    reader,
                    &text_bytes,
                    (max_hits - out.len()).min(per_type),
                );
                for mut h in sub {
                    h.kind = ValueType::Bytes;
                    h.value = format_value(ValueType::Bytes, &text_bytes);
                    out.push(h);
                }
            }

            out
        }
        _ => {
            let mut hits = scan_all_regions(reader, needle, max_hits);
            for h in &mut hits {
                h.kind = kind;
                h.value = format_value(kind, needle);
            }
            hits
        }
    }
}

/// Reinterpret a raw byte needle as the given type. Used only when
/// the user typed a numeric value and selected "all".
fn reinterpret_needle_for(ty: ValueType, needle: &[u8]) -> Option<Vec<u8>> {
    let n = needle.len();
    match ty {
        ValueType::Int32 => {
            if n >= 4 {
                Some(needle[..4].to_vec())
            } else {
                None
            }
        }
        ValueType::Int64 => {
            if n == 4 {
                let v = i32::from_le_bytes([needle[0], needle[1], needle[2], needle[3]]);
                Some((v as i64).to_le_bytes().to_vec())
            } else if n >= 8 {
                Some(needle[..8].to_vec())
            } else {
                None
            }
        }
        ValueType::Float => {
            if n >= 4 {
                Some(needle[..4].to_vec())
            } else {
                None
            }
        }
        ValueType::Double => {
            if n == 4 {
                let v = f32::from_le_bytes([needle[0], needle[1], needle[2], needle[3]]);
                Some((v as f64).to_le_bytes().to_vec())
            } else if n >= 8 {
                Some(needle[..8].to_vec())
            } else {
                None
            }
        }
        ValueType::Int16 => {
            if n >= 2 {
                Some(needle[..2].to_vec())
            } else {
                None
            }
        }
        ValueType::Uint16 => {
            if n >= 2 {
                Some(needle[..2].to_vec())
            } else {
                None
            }
        }
        ValueType::Int8 | ValueType::Uint8 => {
            if n >= 1 {
                Some(needle[..1].to_vec())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Refine an existing hit list by re-reading each address and keeping
/// only the entries that still match `needle`.
pub fn refine_hits(reader: &MemoryReader, hits: &[ScanHit], needle: &[u8]) -> Vec<ScanHit> {
    if needle.is_empty() {
        return hits.to_vec();
    }
    let mut out = Vec::new();
    for hit in hits {
        if let Ok(buf) = reader.read(hit.address, needle.len()) {
            if buf.len() == needle.len() && buf.as_slice() == needle {
                out.push(hit.clone());
            }
        }
    }
    out
}

// ============================================================
// LIVE REFRESH
// ============================================================

/// Re-read a single address in its recorded type.
pub fn read_typed(reader: &MemoryReader, address: u64, kind: ValueType) -> Option<String> {
    let len = match kind {
        ValueType::All => 4,
        ValueType::Bytes | ValueType::String => 16,
        other => other.byte_size().max(1),
    };
    let bytes = reader.read(address, len).ok()?;
    let k = if kind == ValueType::All { ValueType::Bytes } else { kind };
    Some(format_value(k, &bytes))
}

/// Refresh the value of one hit in place. Updates `previous` and
/// `change` based on the new reading.
pub fn refresh_hit(reader: &MemoryReader, hit: &mut ScanHit) {
    let Some(new_val) = read_typed(reader, hit.address, hit.kind) else {
        return;
    };
    if new_val == hit.value {
        hit.previous = Some(hit.value.clone());
        hit.change = ChangeDir::Same;
        return;
    }
    hit.previous = Some(hit.value.clone());
    hit.change = classify_change(&hit.value, &new_val);
    hit.value = new_val;
}

fn classify_change(prev: &str, new: &str) -> ChangeDir {
    if let (Ok(a), Ok(b)) = (prev.parse::<f64>(), new.parse::<f64>()) {
        if b > a {
            ChangeDir::Up
        } else if b < a {
            ChangeDir::Down
        } else {
            ChangeDir::Same
        }
    } else {
        ChangeDir::Same
    }
}

// ============================================================
// FROZEN VALUES
// ============================================================

/// A value pinned to re-write every tick. Lives here instead of in
/// `state.rs` so the script builtins (which live under `core::script`)
/// can reference it without depending on the app module.
#[derive(Clone)]
pub struct FrozenEntry {
    pub address: u64,
    pub value_type: ValueType,
    pub bytes: Vec<u8>,
}