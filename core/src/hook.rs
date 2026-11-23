//! Inline hooks: the first instructions of a function move into a trampoline (relocated by
//! [`crate::x64`]), the function starts with `jmp rel32` to a relay `jmp [rip]` → detour. Code is
//! placed within ±2 GiB of the target; patching happens with every other thread suspended and
//! none of them inside the patched bytes.

use crate::{mem, x64};
use shk_sys::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Executable memory near an address.
struct Chunk {
    base: usize,
    size: usize,
    used: usize,
}

static CHUNKS: Mutex<Vec<Chunk>> = Mutex::new(Vec::new());
const CHUNK: usize = 0x10000;

fn near(a: usize, b: usize) -> bool {
    (a as i64 - b as i64).unsigned_abs() < 0x7FF0_0000
}

/// `len` bytes of RWX memory within ±2 GiB of `addr` (never freed).
pub fn alloc_near(addr: usize, len: usize) -> Option<usize> {
    let len = (len + 15) & !15;
    let mut chunks = CHUNKS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = chunks.iter_mut().find(|c| near(c.base, addr) && near(c.base + c.size, addr) && c.size - c.used >= len) {
        let p = c.base + c.used;
        c.used += len;
        return Some(p);
    }
    // free regions around addr, closest first, 64 KiB aligned
    let lo = addr.saturating_sub(0x7FF0_0000).max(0x10000);
    let hi = addr + 0x7FF0_0000;
    let mut cands: Vec<usize> = Vec::new();
    let mut a = lo & !0xFFFF;
    while a < hi && cands.len() < 64 {
        let Some(m) = mem::query(a) else { break };
        if m.State == MEM_FREE {
            let start = (m.BaseAddress + 0xFFFF) & !0xFFFF;
            if start + CHUNK <= m.BaseAddress + m.RegionSize {
                cands.push(start);
                cands.push((m.BaseAddress + m.RegionSize - CHUNK) & !0xFFFF);
            }
        }
        let next = m.BaseAddress + m.RegionSize;
        if next <= a {
            break;
        }
        a = next;
    }
    cands.sort_by_key(|c| (*c as i64 - addr as i64).unsigned_abs());
    for c in cands {
        if !near(c, addr) || !near(c + CHUNK, addr) {
            continue;
        }
        // SAFETY: reserving free address space.
        let p = unsafe { VirtualAlloc(c as *mut c_void, CHUNK, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) } as usize;
        if p != 0 {
            chunks.push(Chunk { base: p, size: CHUNK, used: len });
            return Some(p);
        }
    }
    None
}

/// Every other thread of the process, suspended until dropped.
struct Frozen(Vec<HANDLE>);

impl Frozen {
    fn all() -> Frozen {
        let mut v = Vec::new();
        // SAFETY: toolhelp snapshot of this process's threads.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap == INVALID_HANDLE_VALUE {
                return Frozen(v);
            }
            let mut te: THREADENTRY32 = std::mem::zeroed();
            te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let (me, pid) = (GetCurrentThreadId(), GetCurrentProcessId());
            let mut ok = Thread32First(snap, &mut te);
            while ok != 0 {
                if te.th32OwnerProcessID == pid && te.th32ThreadID != me {
                    let h = OpenThread(THREAD_ALL_ACCESS, 0, te.th32ThreadID);
                    if !h.is_null() {
                        if SuspendThread(h) != u32::MAX {
                            v.push(h);
                        } else {
                            CloseHandle(h);
                        }
                    }
                }
                ok = Thread32Next(snap, &mut te);
            }
            CloseHandle(snap);
        }
        Frozen(v)
    }

    /// Is any suspended thread's instruction pointer inside [a, a+n)?
    fn inside(&self, a: usize, n: usize) -> bool {
        #[repr(C, align(16))]
        struct Ctx([u8; 1232]);
        for &h in &self.0 {
            let mut c = Box::new(Ctx([0; 1232]));
            c.0[0x30..0x34].copy_from_slice(&CONTEXT_CONTROL.to_le_bytes());
            // SAFETY: an aligned CONTEXT buffer.
            if unsafe { GetThreadContext(h, c.0.as_mut_ptr() as *mut c_void) } != 0 {
                let rip = u64::from_le_bytes(c.0[0xF8..0x100].try_into().unwrap()) as usize;
                if rip >= a && rip < a + n {
                    return true;
                }
            }
        }
        false
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        for &h in &self.0 {
            // SAFETY: our handles.
            unsafe {
                ResumeThread(h);
                CloseHandle(h);
            }
        }
    }
}

/// Writes `bytes` at `addr` with the other threads stopped outside the range (retries a few
/// times when one is inside).
fn patch_quiet(addr: usize, bytes: &[u8]) -> Result<(), String> {
    for _ in 0..50 {
        let f = Frozen::all();
        if !f.inside(addr, bytes.len()) {
            let ok = mem::patch(addr, bytes);
            drop(f);
            return if ok { Ok(()) } else { Err(format!("cannot write at {addr:#x}")) };
        }
        drop(f);
        // SAFETY: plain sleep.
        unsafe { Sleep(1) };
    }
    Err("a thread keeps executing the bytes to patch".into())
}

pub struct Hook {
    pub id: u64,
    pub owner: u64,
    pub target: usize,
    pub trampoline: usize,
    original: Vec<u8>,
}

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Hooks `target` with `detour`; returns (id, trampoline calling the original function).
pub fn install(target: usize, detour: usize, owner: u64) -> Result<(u64, usize), String> {
    install_ex(target, detour, owner, &|_| {})
}

/// As [`install`]; `prepare(trampoline)` runs before the target is patched (to wire a stub).
pub fn install_ex(target: usize, detour: usize, owner: u64, prepare: &dyn Fn(usize)) -> Result<(u64, usize), String> {
    if target == 0 || detour == 0 {
        return Err("null address".into());
    }
    let mut hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
    if hooks.iter().any(|h| h.target == target) {
        return Err(format!("{target:#x} is already hooked"));
    }
    let m = mem::query(target).ok_or("unmapped target")?;
    if m.State != MEM_COMMIT || m.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY) == 0 {
        return Err(format!("{target:#x} is not executable code"));
    }
    let mut code = [0u8; 64];
    if !mem::read(target, &mut code) {
        // near the end of a region: read what is there
        let avail = m.BaseAddress + m.RegionSize - target;
        if avail < 5 || !mem::read(target, &mut code[..avail.min(64)]) {
            return Err("unreadable target".into());
        }
    }
    let insns = x64::cover(&code, 5)?;
    let n: usize = insns.iter().map(|i| i.len).sum();
    let space = alloc_near(target, 14 + n * 4 + 64).ok_or("no free memory within 2 GiB of the target")?;
    // relay: jmp [rip] → detour
    let relay = x64::abs_jmp(detour as u64);
    let tramp_at = space + 16;
    let mut tramp = x64::relocate(&code, &insns, target as u64, tramp_at as u64)?;
    tramp.extend(x64::abs_jmp((target + n) as u64));
    // SAFETY: our fresh RWX memory.
    unsafe {
        std::ptr::copy_nonoverlapping(relay.as_ptr(), space as *mut u8, relay.len());
        std::ptr::copy_nonoverlapping(tramp.as_ptr(), tramp_at as *mut u8, tramp.len());
        FlushInstructionCache(GetCurrentProcess(), space as *const c_void, 16 + tramp.len());
    }
    prepare(tramp_at);
    let rel = space as i64 - (target as i64 + 5);
    let mut jmp = vec![0xE9];
    jmp.extend_from_slice(&(rel as i32).to_le_bytes());
    jmp.resize(n, 0x90);
    patch_quiet(target, &jmp)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    hooks.push(Hook { id, owner, target, trampoline: tramp_at, original: code[..n].to_vec() });
    Ok((id, tramp_at))
}

/// Restores the original bytes (the trampoline stays: a thread may still run it).
pub fn remove(id: u64) -> Result<(), String> {
    let mut hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
    let i = hooks.iter().position(|h| h.id == id).ok_or("no such hook")?;
    patch_quiet(hooks[i].target, &hooks[i].original)?;
    hooks.remove(i);
    Ok(())
}

pub fn remove_owner(owner: u64) -> usize {
    let ids: Vec<u64> = HOOKS.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|h| h.owner == owner).map(|h| h.id).collect();
    ids.into_iter().filter(|&id| remove(id).is_ok()).count()
}

pub fn count() -> usize {
    HOOKS.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// A "call before" stub: saves the four integer and four float argument registers, calls
/// `before()` (no arguments), restores them and jumps to `then` (the trampoline). Used to run
/// scripts from the game's script tick without touching its arguments.
pub fn call_before_stub(near_addr: usize, before: usize) -> Option<(usize, usize)> {
    let mut c: Vec<u8> = vec![
        0x51, 0x52, 0x41, 0x50, 0x41, 0x51, // push rcx, rdx, r8, r9
        0x48, 0x83, 0xEC, 0x68, // sub rsp, 0x68 (shadow space + 4 xmm, 16-byte aligned)
        0xF3, 0x0F, 0x7F, 0x44, 0x24, 0x20, // movdqu [rsp+0x20], xmm0
        0xF3, 0x0F, 0x7F, 0x4C, 0x24, 0x30, // movdqu [rsp+0x30], xmm1
        0xF3, 0x0F, 0x7F, 0x54, 0x24, 0x40, // movdqu [rsp+0x40], xmm2
        0xF3, 0x0F, 0x7F, 0x5C, 0x24, 0x50, // movdqu [rsp+0x50], xmm3
        0x48, 0xB8, // mov rax, imm64
    ];
    c.extend_from_slice(&(before as u64).to_le_bytes());
    c.extend_from_slice(&[
        0xFF, 0xD0, // call rax
        0xF3, 0x0F, 0x6F, 0x44, 0x24, 0x20, // movdqu xmm0, [rsp+0x20]
        0xF3, 0x0F, 0x6F, 0x4C, 0x24, 0x30,
        0xF3, 0x0F, 0x6F, 0x54, 0x24, 0x40,
        0xF3, 0x0F, 0x6F, 0x5C, 0x24, 0x50,
        0x48, 0x83, 0xC4, 0x68, // add rsp, 0x68
        0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, // pop r9, r8, rdx, rcx
        0xFF, 0x25, 0, 0, 0, 0, // jmp [rip]
    ]);
    let slot = c.len();
    c.extend_from_slice(&0u64.to_le_bytes()); // the trampoline, written later
    let at = alloc_near(near_addr, c.len())?;
    // SAFETY: our RWX memory.
    unsafe { std::ptr::copy_nonoverlapping(c.as_ptr(), at as *mut u8, c.len()) };
    Some((at, at + slot))
}

/// Points a stub's final jump at `to`.
pub fn set_stub_target(slot: usize, to: usize) {
    // SAFETY: our RWX memory.
    unsafe { std::ptr::write_unaligned(slot as *mut u64, to as u64) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[inline(never)]
    extern "C" fn add(a: i32, b: i32) -> i32 {
        std::hint::black_box(a).wrapping_mul(std::hint::black_box(1)) + std::hint::black_box(b)
    }

    static ORIG: AtomicU64 = AtomicU64::new(0);
    extern "C" fn add_detour(a: i32, b: i32) -> i32 {
        let o: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(ORIG.load(Ordering::SeqCst) as usize) };
        o(a, b) * 10
    }

    #[test]
    fn hook_a_compiled_function() {
        let f: extern "C" fn(i32, i32) -> i32 = std::hint::black_box(add);
        assert_eq!(f(2, 3), 5);
        let (id, tramp) = install(add as *const () as usize, add_detour as *const () as usize, 7).unwrap();
        ORIG.store(tramp as u64, Ordering::SeqCst);
        assert_eq!(f(2, 3), 50);
        assert!(install(add as *const () as usize, add_detour as *const () as usize, 7).is_err(), "already hooked");
        remove(id).unwrap();
        assert_eq!(f(2, 3), 5);
    }

    /// Generated code with a RIP-relative load and a branch in its first bytes.
    #[test]
    fn hook_relocates_rip_relative_and_branches() {
        // mov eax, [rip+0x20]; test ecx, ecx; jz +3; add eax, ecx; ret; nop...; data at +0x26
        let mut code = vec![0x8B, 0x05, 0x20, 0, 0, 0, 0x85, 0xC9, 0x74, 0x02, 0x01, 0xC8, 0xC3];
        code.resize(0x26, 0x90);
        code.extend_from_slice(&100u32.to_le_bytes());
        let base = alloc_near(add as *const () as usize, 0x40).unwrap();
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), base as *mut u8, code.len()) };
        let f: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(base) };
        assert_eq!(f(5), 105);
        assert_eq!(f(0), 100);
        static T: AtomicU64 = AtomicU64::new(0);
        extern "C" fn det(x: i32) -> i32 {
            let o: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(T.load(Ordering::SeqCst) as usize) };
            o(x) + 1
        }
        let (id, tr) = install(base, det as *const () as usize, 1).unwrap();
        T.store(tr as u64, Ordering::SeqCst);
        assert_eq!(f(5), 106);
        assert_eq!(f(0), 101);
        remove(id).unwrap();
        assert_eq!(f(5), 105);
    }

    #[test]
    fn call_before_keeps_arguments() {
        static CALLS: AtomicU32 = AtomicU32::new(0);
        extern "C" fn before() {
            CALLS.fetch_add(1, Ordering::SeqCst);
        }
        #[inline(never)]
        extern "C" fn mix(a: i64, b: f64, c: i64, d: f32) -> f64 {
            std::hint::black_box(a) as f64 + b * 2.0 + c as f64 * 3.0 + d as f64 * 4.0
        }
        let f: extern "C" fn(i64, f64, i64, f32) -> f64 = std::hint::black_box(mix);
        let want = f(1, 2.5, 3, 4.5);
        let (stub, slot) = call_before_stub(mix as *const () as usize, before as *const () as usize).unwrap();
        let (id, _) = install_ex(mix as *const () as usize, stub, 2, &|t| set_stub_target(slot, t)).unwrap();
        assert_eq!(f(1, 2.5, 3, 4.5), want);
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        remove(id).unwrap();
    }
}
