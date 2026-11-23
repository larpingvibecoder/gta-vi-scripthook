//! Plugin runtime: the host pointer, the plugin's identity, load / unload, panic guards and the
//! storage of callbacks handed to the host.

use crate::ffi::{ShkHost, ShkPlugin, SHK_ABI_VERSION};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;

static HOST: AtomicPtr<ShkHost> = AtomicPtr::new(std::ptr::null_mut());
static SELF: AtomicPtr<ShkPlugin> = AtomicPtr::new(std::ptr::null_mut());
static UNLOADING: AtomicBool = AtomicBool::new(false);

pub fn host_ptr() -> *const ShkHost {
    HOST.load(Ordering::Acquire)
}

pub fn self_ptr() -> *mut ShkPlugin {
    SELF.load(Ordering::Acquire)
}

/// True once the plugin is being unloaded: callbacks return at once.
pub fn unloading() -> bool {
    UNLOADING.load(Ordering::Acquire)
}

/// Sets the host (tests and custom entry points; `export_plugin!` does it for you).
///
/// # Safety
/// `host` must stay valid for as long as the SDK is used.
pub unsafe fn set_host(host: *const ShkHost, me: *mut ShkPlugin) {
    HOST.store(host as *mut ShkHost, Ordering::Release);
    SELF.store(me, Ordering::Release);
    UNLOADING.store(false, Ordering::Release);
}

/// Moves `v` to the heap for the rest of the process (callback state the host points at; it is
/// never freed so no callback can see freed memory, even one running while the plugin unloads).
pub fn leak<T>(v: T) -> *mut T {
    Box::into_raw(Box::new(v))
}

/// Runs `f`, turning a panic into a log line. Every callback the SDK hands to the host goes
/// through here: no unwinding ever crosses into the host.
pub fn guard<R>(what: &str, f: impl FnOnce() -> R) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(e) => {
            let msg = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
            if !host_ptr().is_null() {
                crate::host().log(crate::ffi::SHK_LOG_ERROR, &format!("{what}: {msg}"));
            }
            None
        }
    }
}

/// `shk_plugin_load` body for [`crate::Plugin`] types.
///
/// # Safety
/// Called by the host (or the adapter) with valid pointers.
pub unsafe fn load<P: crate::Plugin>(host: *const ShkHost, me: *mut ShkPlugin, slot: &Mutex<Option<P>>) -> i32 {
    if host.is_null() || (*host).abi_version != SHK_ABI_VERSION || (*host).core.is_null() {
        return 1;
    }
    set_host(host, me);
    match guard("load", || P::load(crate::host())) {
        Some(Ok(p)) => {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
            0
        }
        Some(Err(e)) => {
            crate::host().log(crate::ffi::SHK_LOG_ERROR, &format!("not loaded: {e}"));
            2
        }
        None => 3,
    }
}

/// `shk_plugin_unload` body.
///
/// # Safety
/// Called by the host once.
pub unsafe fn unload<P: crate::Plugin>(slot: &Mutex<Option<P>>) {
    let p = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(mut p) = p {
        guard("unload", || p.unload());
        // the plugin's own state is dropped here; callbacks registered with the host are
        // not called any more from now on
        UNLOADING.store(true, Ordering::Release);
        guard("drop", || drop(p));
    }
    UNLOADING.store(true, Ordering::Release);
}
