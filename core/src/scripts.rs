//! Script functions on fibers. Every script tick (the game's script thread, through the hook of
//! its tick function; or before each presented frame as a fallback) the ticking thread becomes a
//! fiber, switches to each script whose wake time has come, and the script runs until it waits
//! or returns.

use shk_sys::*;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

pub type ScriptFn = unsafe extern "C" fn(*mut c_void);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off = 0,
    GameThread = 1,
    PresentFallback = 2,
}

struct Script {
    id: u32,
    owner: u64,
    name: String,
    fiber: *mut c_void,
    wake: f64,
    done: bool,
    removed: bool,
    f: ScriptFn,
    user: usize,
    runs: u64,
}

struct List(Vec<Box<Script>>);
// SAFETY: fibers are only switched to from the ticking thread under TICK.
unsafe impl Send for List {}

static SCRIPTS: Mutex<List> = Mutex::new(List(Vec::new()));
static TICK: Mutex<()> = Mutex::new(());
static NEXT: AtomicU32 = AtomicU32::new(1);
static MODE: AtomicU8 = AtomicU8::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);
static TICK_THREAD: AtomicU32 = AtomicU32::new(0);
static REASON: Mutex<String> = Mutex::new(String::new());
static COUNT: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static CURRENT: Cell<*mut Script> = const { Cell::new(std::ptr::null_mut()) };
    static MAIN: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
}

fn list() -> std::sync::MutexGuard<'static, List> {
    SCRIPTS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_mode(m: Mode, reason: &str) {
    MODE.store(m as u8, Ordering::Release);
    *REASON.lock().unwrap_or_else(|e| e.into_inner()) = reason.to_string();
}

pub fn mode() -> Mode {
    match MODE.load(Ordering::Acquire) {
        1 => Mode::GameThread,
        2 => Mode::PresentFallback,
        _ => Mode::Off,
    }
}

pub fn reason() -> String {
    REASON.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn tick_thread() -> u32 {
    TICK_THREAD.load(Ordering::Relaxed)
}

pub fn count() -> u32 {
    COUNT.load(Ordering::Relaxed)
}

pub fn in_script() -> bool {
    CURRENT.with(|c| !c.get().is_null())
}

/// The current fiber (TEB NtTib.FiberData) of a thread that already is one.
fn current_fiber() -> *mut c_void {
    let f: usize;
    // SAFETY: reads gs:[0x20], the fiber data of the current thread's TIB.
    unsafe { std::arch::asm!("mov {}, gs:[0x20]", out(reg) f, options(nostack, readonly, preserves_flags)) };
    f as *mut c_void
}

unsafe extern "system" fn fiber_main(p: *mut c_void) {
    let s = p as *mut Script;
    ((*s).f)((*s).user as *mut c_void);
    (*s).done = true;
    loop {
        SwitchToFiber(MAIN.with(|m| m.get()));
    }
}

/// Registers a script (runs from the next tick). Returns its id.
pub fn register(owner: u64, name: &str, f: ScriptFn, user: usize) -> u32 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    list().0.push(Box::new(Script { id, owner, name: name.to_string(), fiber: std::ptr::null_mut(), wake: 0.0, done: false, removed: false, f, user, runs: 0 }));
    COUNT.fetch_add(1, Ordering::Relaxed);
    id
}

/// Inside a script: yields for at least `ms` (0 = next tick). False outside a script.
pub fn wait(ms: u32) -> bool {
    let s = CURRENT.with(|c| c.get());
    if s.is_null() {
        return false;
    }
    // SAFETY: the running script's record (boxed, alive while it runs).
    unsafe {
        (*s).wake = now() + ms as f64 / 1000.0;
        SwitchToFiber(MAIN.with(|m| m.get()));
    }
    true
}

fn reap(l: &mut List) {
    l.0.retain(|s| {
        let gone = s.done || s.removed;
        if gone {
            if !s.fiber.is_null() {
                // SAFETY: a fiber that is not running.
                unsafe { DeleteFiber(s.fiber) };
            }
            COUNT.fetch_sub(1, Ordering::Relaxed);
        }
        !gone
    });
}

/// Removes one of `owner`'s scripts. Called by the script itself, it stops at its next wait.
pub fn unregister(owner: u64, id: u32) -> bool {
    let mut l = list();
    match l.0.iter_mut().find(|s| s.id == id && s.owner == owner) {
        Some(s) => {
            s.removed = true;
            true
        }
        None => false,
    }
}

/// Removes every script of `owner`, waiting for a tick in progress (unless called from one).
pub fn remove_owner(owner: u64) -> usize {
    let n;
    if in_script() {
        let mut l = list();
        n = l.0.iter_mut().filter(|s| s.owner == owner).map(|s| s.removed = true).count();
    } else {
        let _t = TICK.lock().unwrap_or_else(|e| e.into_inner());
        let mut l = list();
        n = l.0.iter_mut().filter(|s| s.owner == owner).map(|s| s.removed = true).count();
        reap(&mut l);
    }
    n
}

/// One script tick on the calling thread.
pub fn tick(m: Mode) {
    if mode() != m || COUNT.load(Ordering::Relaxed) == 0 || in_script() {
        return;
    }
    let Ok(_t) = TICK.try_lock() else { return };
    TICKS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: plain thread id.
    TICK_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    // SAFETY: fiber conversion of the calling thread, undone below.
    let converted = unsafe { IsThreadAFiber() == 0 };
    let main = if converted { unsafe { ConvertThreadToFiber(std::ptr::null_mut()) } } else { current_fiber() };
    if main.is_null() {
        return;
    }
    MAIN.with(|c| c.set(main));
    let t = now();
    let due: Vec<*mut Script> = {
        let mut l = list();
        l.0.iter_mut().filter(|s| !s.done && !s.removed && s.wake <= t).map(|s| &mut **s as *mut Script).collect()
    };
    for s in due {
        // SAFETY: records stay boxed until reaped (below, by this thread, or under TICK).
        unsafe {
            if (*s).removed {
                continue;
            }
            if (*s).fiber.is_null() {
                (*s).fiber = CreateFiber(256 * 1024, fiber_main, s as *mut c_void);
                if (*s).fiber.is_null() {
                    (*s).done = true;
                    continue;
                }
            }
            (*s).runs += 1;
            CURRENT.with(|c| c.set(s));
            SwitchToFiber((*s).fiber);
            CURRENT.with(|c| c.set(std::ptr::null_mut()));
        }
    }
    reap(&mut list());
    if converted {
        // SAFETY: we converted it above.
        unsafe { ConvertFiberToThread() };
    }
}

/// (id, owner, name, runs) of every script.
pub fn snapshot() -> Vec<(u32, u64, String, u64)> {
    list().0.iter().map(|s| (s.id, s.owner, s.name.clone(), s.runs)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    static A: AtomicI32 = AtomicI32::new(0);
    static B: AtomicI32 = AtomicI32::new(0);

    unsafe extern "C" fn counting(_: *mut c_void) {
        loop {
            A.fetch_add(1, Ordering::SeqCst);
            assert!(in_script());
            wait(0);
        }
    }
    unsafe extern "C" fn three_then_return(_: *mut c_void) {
        for _ in 0..3 {
            B.fetch_add(1, Ordering::SeqCst);
            wait(0);
        }
    }

    #[test]
    fn fibers_on_a_ticking_thread() {
        let _g = crate::TEST_SCRIPTS.lock().unwrap_or_else(|e| e.into_inner());
        set_mode(Mode::GameThread, "");
        assert!(!wait(0), "no wait outside scripts");
        let a = register(1, "a", counting, 0);
        register(2, "b", three_then_return, 0);
        let th = std::thread::spawn(|| {
            for _ in 0..10 {
                tick(Mode::GameThread);
            }
            tick(Mode::PresentFallback); // wrong mode: nothing
        });
        th.join().unwrap();
        assert_eq!(A.load(Ordering::SeqCst), 10);
        assert_eq!(B.load(Ordering::SeqCst), 3);
        assert!(snapshot().iter().all(|s| s.1 != 2), "returned script removed");
        assert!(unregister(1, a));
        tick(Mode::GameThread);
        assert_eq!(A.load(Ordering::SeqCst), 10, "removed script never resumed");
        assert_eq!(remove_owner(1), 0);
        assert_eq!(count(), 0);
        set_mode(Mode::Off, "");
    }
}
