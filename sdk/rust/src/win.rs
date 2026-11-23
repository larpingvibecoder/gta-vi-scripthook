//! The few Windows functions the SDK uses (kernel32; declarations from Microsoft Learn).

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const PAGE_READWRITE: u32 = 0x04;
pub const FILE_MAP_ALL_ACCESS: u32 = 0x000F_001F;

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateFileMappingW(file: HANDLE, attrs: *mut c_void, protect: u32, size_high: u32, size_low: u32, name: *const u16) -> HANDLE;
    pub fn MapViewOfFile(map: HANDLE, access: u32, off_high: u32, off_low: u32, bytes: usize) -> *mut c_void;
    pub fn UnmapViewOfFile(base: *const c_void) -> i32;
    pub fn CloseHandle(h: HANDLE) -> i32;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetCurrentThreadId() -> u32;
    pub fn QueryPerformanceCounter(out: *mut i64) -> i32;
    pub fn QueryPerformanceFrequency(out: *mut i64) -> i32;
}

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Seconds from a monotonic clock.
pub fn now() -> f64 {
    let (mut c, mut f) = (0i64, 0i64);
    // SAFETY: out parameters on the stack.
    unsafe {
        QueryPerformanceCounter(&mut c);
        QueryPerformanceFrequency(&mut f);
    }
    if f == 0 {
        0.0
    } else {
        c as f64 / f as f64
    }
}
