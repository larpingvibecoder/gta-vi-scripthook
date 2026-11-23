//! The service table the version-2 adapter emulates: a process-local named shared memory section
//! ("Local\gta-vi-scripthook-services-1-<pid>"), 4 KiB, shared by every plugin built with this
//! SDK or `sdk/c/scripthook_srt2.c` (same layout):
//!
//! ```text
//! 0x00 lock (i32, 0 = free)   0x04 magic "SHK1"   0x08 count   0x0C reserved
//! 0x10 63 entries of { name[48] (NUL-terminated), iface (u64), owner (u64) }
//! ```

use crate::ffi::ShkPlugin;
use crate::win;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

const MAGIC: u32 = 0x5348_4B31;
const MAX: usize = 63;

#[repr(C)]
struct Entry {
    name: [u8; 48],
    iface: u64,
    owner: u64,
}

#[repr(C)]
struct Table {
    lock: AtomicI32,
    magic: u32,
    count: u32,
    reserved: u32,
    entries: [Entry; MAX],
}

static TABLE: AtomicPtr<Table> = AtomicPtr::new(std::ptr::null_mut());
static MAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn table() -> Option<&'static mut Table> {
    let t = TABLE.load(Ordering::Acquire);
    if !t.is_null() {
        // SAFETY: mapped below, valid until `close`.
        return Some(unsafe { &mut *t });
    }
    let name = win::wide(&format!("Local\\gta-vi-scripthook-services-1-{}", unsafe { win::GetCurrentProcessId() }));
    // SAFETY: documented kernel32 calls; a pagefile-backed section of 4 KiB.
    unsafe {
        let m = win::CreateFileMappingW(win::INVALID_HANDLE_VALUE, std::ptr::null_mut(), win::PAGE_READWRITE, 0, 4096, name.as_ptr());
        if m.is_null() {
            return None;
        }
        let v = win::MapViewOfFile(m, win::FILE_MAP_ALL_ACCESS, 0, 0, 4096) as *mut Table;
        if v.is_null() {
            win::CloseHandle(m);
            return None;
        }
        match TABLE.compare_exchange(std::ptr::null_mut(), v, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => MAP.store(m, Ordering::Release),
            Err(other) => {
                win::UnmapViewOfFile(v as *const c_void);
                win::CloseHandle(m);
                return Some(&mut *other);
            }
        }
        Some(&mut *v)
    }
}

struct Guard(&'static mut Table);

impl Guard {
    fn lock() -> Option<Guard> {
        let t = table()?;
        while t.lock.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            std::hint::spin_loop();
        }
        if t.magic != MAGIC {
            // SAFETY: plain bytes.
            unsafe { std::ptr::write_bytes(t.entries.as_mut_ptr(), 0, MAX) };
            t.count = 0;
            t.magic = MAGIC;
        }
        Some(Guard(t))
    }
    fn find(&self, name: &[u8]) -> Option<usize> {
        (0..self.0.count as usize).find(|&i| cname(&self.0.entries[i].name) == name)
    }
    fn remove_at(&mut self, i: usize) {
        let last = self.0.count as usize - 1;
        self.0.entries.swap(i, last);
        self.0.entries[last] = Entry { name: [0; 48], iface: 0, owner: 0 };
        self.0.count -= 1;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.lock.store(0, Ordering::Release);
    }
}

fn cname(b: &[u8; 48]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(48);
    &b[..n]
}

pub(crate) unsafe extern "C" fn publish(me: *mut ShkPlugin, name: *const c_char, iface: *const c_void) -> i32 {
    if name.is_null() || iface.is_null() {
        return 0;
    }
    let n = CStr::from_ptr(name).to_bytes();
    if n.is_empty() || n.len() >= 48 {
        return 0;
    }
    let Some(g) = Guard::lock() else { return 0 };
    let i = match g.find(n) {
        Some(i) if g.0.entries[i].owner != me as u64 => return 0,
        Some(i) => i,
        None if (g.0.count as usize) < MAX => {
            g.0.count += 1;
            g.0.count as usize - 1
        }
        None => return 0,
    };
    let e = &mut g.0.entries[i];
    e.name = [0; 48];
    e.name[..n.len()].copy_from_slice(n);
    e.iface = iface as u64;
    e.owner = me as u64;
    1
}

pub(crate) unsafe extern "C" fn withdraw(me: *mut ShkPlugin, name: *const c_char) -> i32 {
    if name.is_null() {
        return 0;
    }
    let n = CStr::from_ptr(name).to_bytes();
    let Some(mut g) = Guard::lock() else { return 0 };
    match g.find(n) {
        Some(i) if g.0.entries[i].owner == me as u64 => {
            g.remove_at(i);
            1
        }
        _ => 0,
    }
}

pub(crate) unsafe extern "C" fn find(name: *const c_char) -> *const c_void {
    if name.is_null() {
        return std::ptr::null();
    }
    let n = CStr::from_ptr(name).to_bytes();
    let Some(g) = Guard::lock() else { return std::ptr::null() };
    g.find(n).map(|i| g.0.entries[i].iface as *const c_void).unwrap_or(std::ptr::null())
}

/// Withdraws everything `me` published and unmaps the table (plugin unload).
pub(crate) fn close(me: *mut ShkPlugin) {
    if TABLE.load(Ordering::Acquire).is_null() {
        return;
    }
    if let Some(mut g) = Guard::lock() {
        let mut i = 0;
        while i < g.0.count as usize {
            if g.0.entries[i].owner == me as u64 {
                g.remove_at(i);
            } else {
                i += 1;
            }
        }
    }
    let t = TABLE.swap(std::ptr::null_mut(), Ordering::AcqRel);
    let m = MAP.swap(std::ptr::null_mut(), Ordering::AcqRel);
    // SAFETY: our own view and handle.
    unsafe {
        if !t.is_null() {
            win::UnmapViewOfFile(t as *const c_void);
        }
        if !m.is_null() {
            win::CloseHandle(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_find_withdraw() {
        let a = 0x1000 as *mut ShkPlugin;
        let b = 0x2000 as *mut ShkPlugin;
        let x = 5u32;
        let y = 6u32;
        unsafe {
            assert_eq!(publish(a, c"test.service".as_ptr(), &x as *const u32 as *const c_void), 1);
            assert_eq!(find(c"test.service".as_ptr()), &x as *const u32 as *const c_void);
            // another owner cannot take the name
            assert_eq!(publish(b, c"test.service".as_ptr(), &y as *const u32 as *const c_void), 0);
            assert_eq!(withdraw(b, c"test.service".as_ptr()), 0);
            assert_eq!(publish(b, c"test.other".as_ptr(), &y as *const u32 as *const c_void), 1);
            assert_eq!(withdraw(a, c"test.service".as_ptr()), 1);
            assert!(find(c"test.service".as_ptr()).is_null());
            assert_eq!(find(c"test.other".as_ptr()), &y as *const u32 as *const c_void);
        }
        close(b);
        unsafe { assert!(find(c"test.other".as_ptr()).is_null()) };
        close(a);
    }
}
