//! Safe wrappers over the host tables.

use crate::ffi::*;
use crate::rt;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;

/// The host, valid while the plugin is loaded. Get it with [`crate::host`].
#[derive(Clone, Copy)]
pub struct Host {
    raw: &'static ShkHost,
}

/// A guest module.
#[derive(Debug, Clone)]
pub struct Module {
    pub base: u64,
    pub size: u64,
    pub name: String,
    pub is_title: bool,
    pub sections: u32,
}

/// Where script functions run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMode {
    Off,
    GameThread,
    PresentFallback,
}

#[derive(Debug, Clone)]
pub struct ScriptStatus {
    pub mode: ScriptMode,
    pub scripts: u32,
    pub ticks: u64,
    pub tick_thread: u32,
    pub in_script: bool,
    pub reason: String,
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// True when the group `t` is present and covers `end` bytes.
fn covers<T>(t: *const T, end: usize) -> bool {
    // SAFETY: every group starts with its u32 size.
    !t.is_null() && unsafe { *(t as *const u32) } as usize >= end
}

macro_rules! has {
    ($t:expr, $ty:ty, $m:ident) => {
        covers($t, std::mem::offset_of!($ty, $m) + std::mem::size_of::<usize>())
    };
}

impl Host {
    /// # Safety
    /// `raw` must stay valid while the returned host is used.
    pub unsafe fn from_raw(raw: *const ShkHost) -> Host {
        Host { raw: &*raw }
    }

    pub fn raw(&self) -> &'static ShkHost {
        self.raw
    }

    pub fn name(&self) -> String {
        // SAFETY: host strings are NUL-terminated or null.
        unsafe { format!("{} ({})", text(self.raw.host_name), text(self.raw.host_version)) }
    }

    fn me(&self) -> *mut ShkPlugin {
        rt::self_ptr()
    }

    fn core(&self) -> &ShkCoreApi {
        // SAFETY: core is required (checked at load).
        unsafe { &*self.raw.core }
    }

    // ---- core ---------------------------------------------------------------------------------

    pub fn log(&self, level: i32, msg: &str) {
        let m = c(msg);
        // SAFETY: valid pointers.
        unsafe { (self.core().log)(self.me(), level, m.as_ptr()) }
    }
    pub fn info(&self, msg: &str) {
        self.log(SHK_LOG_INFO, msg)
    }
    pub fn warn(&self, msg: &str) {
        self.log(SHK_LOG_WARN, msg)
    }
    pub fn error(&self, msg: &str) {
        self.log(SHK_LOG_ERROR, msg)
    }
    pub fn notify(&self, msg: &str, seconds: f32) {
        let m = c(msg);
        // SAFETY: valid pointers.
        unsafe { (self.core().notify)(self.me(), m.as_ptr(), seconds) }
    }
    pub fn title_identity(&self) -> String {
        // SAFETY: host-owned string.
        unsafe { text((self.core().title_identity)()) }
    }
    /// The plugin's mod folder.
    pub fn plugin_folder(&self) -> PathBuf {
        // SAFETY: host-owned string.
        PathBuf::from(unsafe { text((self.core().plugin_folder)(self.me())) })
    }

    // ---- memory -------------------------------------------------------------------------------

    fn mem(&self) -> &ShkMemoryApi {
        // SAFETY: memory is part of every host.
        unsafe { &*self.raw.memory }
    }

    pub fn read_bytes(&self, addr: u64, out: &mut [u8]) -> bool {
        // SAFETY: bounds-checked by the host.
        unsafe { (self.mem().read)(addr, out.as_mut_ptr() as *mut c_void, out.len()) != 0 }
    }
    /// Reads a plain value (integers, floats, `#[repr(C)]` structs of them).
    pub fn read<T: Copy>(&self, addr: u64) -> Option<T> {
        let mut v = std::mem::MaybeUninit::<T>::uninit();
        // SAFETY: the host fills all bytes or fails.
        let ok = unsafe { (self.mem().read)(addr, v.as_mut_ptr() as *mut c_void, std::mem::size_of::<T>()) != 0 };
        // SAFETY: filled on success; T: Copy is plain data by contract of this function.
        ok.then(|| unsafe { v.assume_init() })
    }
    pub fn write_bytes(&self, addr: u64, data: &[u8]) -> bool {
        // SAFETY: bounds-checked by the host.
        unsafe { (self.mem().write)(addr, data.as_ptr() as *const c_void, data.len()) != 0 }
    }
    pub fn write<T: Copy>(&self, addr: u64, v: &T) -> bool {
        // SAFETY: reads size_of::<T>() bytes of `v`.
        unsafe { (self.mem().write)(addr, v as *const T as *const c_void, std::mem::size_of::<T>()) != 0 }
    }
    /// Code / read-only data of guest modules.
    pub fn patch(&self, addr: u64, data: &[u8]) -> bool {
        // SAFETY: checked by the host.
        unsafe { (self.mem().patch)(addr, data.as_ptr() as *const c_void, data.len()) != 0 }
    }
    pub fn query(&self, addr: u64) -> Option<ShkRegion> {
        let mut r = ShkRegion::default();
        // SAFETY: out parameter.
        (unsafe { (self.mem().query)(addr, &mut r) } != 0).then_some(r)
    }
    /// `""` = the title image.
    pub fn module(&self, name: &str) -> Option<Module> {
        let n = c(name);
        // SAFETY: zeroed plain struct as out parameter.
        let mut m: ShkModule = unsafe { std::mem::zeroed() };
        (unsafe { (self.mem().module)(n.as_ptr(), &mut m) } != 0).then(|| Module { base: m.base, size: m.size, name: fixed_str(&m.name), is_title: m.is_title != 0, sections: m.sections })
    }
    pub fn modules(&self) -> Vec<Module> {
        unsafe extern "C" fn each(user: *mut c_void, m: *const ShkModule) {
            let v = &mut *(user as *mut Vec<Module>);
            let m = &*m;
            v.push(Module { base: m.base, size: m.size, name: fixed_str(&m.name), is_title: m.is_title != 0, sections: m.sections });
        }
        let mut v: Vec<Module> = Vec::new();
        // SAFETY: the callback runs before `modules` returns.
        unsafe { (self.mem().modules)(Some(each), &mut v as *mut Vec<Module> as *mut c_void) };
        v
    }
    pub fn find_export(&self, module: &str, symbol: &str) -> Option<u64> {
        let (m, s) = (c(module), c(symbol));
        // SAFETY: valid strings.
        let a = unsafe { (self.mem().find_export)(m.as_ptr(), s.as_ptr()) };
        (a != 0).then_some(a)
    }
    pub fn section(&self, module: &str, name: &str) -> Option<(u64, u64)> {
        let (m, n) = (c(module), c(name));
        let (mut a, mut s) = (0u64, 0u64);
        // SAFETY: out parameters.
        (unsafe { (self.mem().section)(m.as_ptr(), n.as_ptr(), &mut a, &mut s) } != 0).then_some((a, s))
    }
    /// The `nth` match of `pattern` ("48 8B ?? ..") in a module (section None = all).
    pub fn scan(&self, module: &str, section: Option<&str>, pattern: &str, nth: u32) -> Option<u64> {
        let (m, p) = (c(module), c(pattern));
        let s = section.map(c);
        // SAFETY: valid strings.
        let a = unsafe { (self.mem().scan)(m.as_ptr(), s.as_ref().map(|x| x.as_ptr()).unwrap_or(std::ptr::null()), p.as_ptr(), nth) };
        (a != 0).then_some(a)
    }
    pub fn scan_all(&self, module: &str, section: Option<&str>, pattern: &str, max: usize) -> Vec<u64> {
        let (m, p) = (c(module), c(pattern));
        let s = section.map(c);
        let mut out = vec![0u64; max.max(1)];
        // SAFETY: `out` holds `max` entries.
        let n = unsafe { (self.mem().scan_all)(m.as_ptr(), s.as_ref().map(|x| x.as_ptr()).unwrap_or(std::ptr::null()), p.as_ptr(), out.as_mut_ptr(), max) };
        out.truncate(n.min(max));
        out
    }
    pub fn rip_target(&self, insn: u64, disp_offset: u32, insn_len: u32) -> Option<u64> {
        // SAFETY: plain values.
        let a = unsafe { (self.mem().rip_target)(insn, disp_offset, insn_len) };
        (a != 0).then_some(a)
    }
    pub fn resolve_chain(&self, base: u64, offsets: &[i64]) -> Option<u64> {
        // SAFETY: slice pointer and length.
        let a = unsafe { (self.mem().resolve_chain)(base, offsets.as_ptr(), offsets.len()) };
        (a != 0).then_some(a)
    }

    // ---- hooks --------------------------------------------------------------------------------

    /// Inline hook: returns the handle and the trampoline calling the original.
    ///
    /// # Safety
    /// `detour` must be an `extern "C"` function with the target's exact signature.
    pub unsafe fn hook(&self, target: u64, detour: *const c_void) -> Option<(ShkHook, *const c_void)> {
        let mut orig: *mut c_void = std::ptr::null_mut();
        let h = ((*self.raw.hooks).install)(self.me(), target, detour as *mut c_void, &mut orig);
        (h != 0).then_some((h, orig as *const c_void))
    }
    pub fn unhook(&self, h: ShkHook) -> bool {
        // SAFETY: plain values.
        unsafe { ((*self.raw.hooks).remove)(self.me(), h) != 0 }
    }

    // ---- input and frames ---------------------------------------------------------------------

    fn input(&self) -> &ShkInputApi {
        // SAFETY: input is part of every host.
        unsafe { &*self.raw.input }
    }

    /// Once per presented frame (frame thread).
    pub fn on_frame(&self, f: impl FnMut(&ShkFrameInfo) + Send + 'static) -> bool {
        unsafe extern "C" fn tramp(user: *mut c_void, info: *const ShkFrameInfo) {
            if rt::unloading() {
                return;
            }
            let f = &mut *(user as *mut Box<dyn FnMut(&ShkFrameInfo) + Send>);
            rt::guard("frame callback", || f(&*info));
        }
        let b: Box<dyn FnMut(&ShkFrameInfo) + Send> = Box::new(f);
        let p = rt::leak(b);
        // SAFETY: leaked state outlives the registration.
        unsafe { (self.input().on_frame)(self.me(), Some(tramp), p as *mut c_void) != 0 }
    }

    /// A hotkey (virtual-key code + `SHK_MOD_*`), menu closed.
    pub fn hotkey(&self, vk: u32, mods: u32, f: impl FnMut() + Send + 'static) -> bool {
        let p = event_box(f);
        // SAFETY: leaked state outlives the registration.
        unsafe { (self.input().hotkey)(self.me(), vk, mods, Some(event_tramp), p) != 0 }
    }

    /// Fires when all `buttons` (`SHK_PAD_*`) become held.
    pub fn pad_chord(&self, buttons: u32, f: impl FnMut() + Send + 'static) -> bool {
        let p = event_box(f);
        // SAFETY: as above.
        unsafe { (self.input().pad_chord)(self.me(), buttons, Some(event_tramp), p) != 0 }
    }

    pub fn key_down(&self, vk: u32) -> bool {
        // SAFETY: plain value.
        unsafe { (self.input().key_down)(vk) != 0 }
    }

    pub fn pad(&self, user: u32) -> Option<ShkPad> {
        let mut p = ShkPad::default();
        // SAFETY: out parameter.
        (unsafe { (self.input().pad)(user, &mut p) } != 0).then_some(p)
    }

    /// Keyboard events (input group version 2). False when the host cannot deliver them.
    pub fn on_key(&self, f: impl FnMut(&ShkKeyEvent) -> bool + Send + 'static) -> bool {
        if !has!(self.raw.input, ShkInputApi, on_key) {
            return false;
        }
        unsafe extern "C" fn tramp(user: *mut c_void, ev: *const ShkKeyEvent) -> i32 {
            if rt::unloading() {
                return 0;
            }
            let f = &mut *(user as *mut Box<dyn FnMut(&ShkKeyEvent) -> bool + Send>);
            rt::guard("key callback", || f(&*ev)).unwrap_or(false) as i32
        }
        let b: Box<dyn FnMut(&ShkKeyEvent) -> bool + Send> = Box::new(f);
        let p = rt::leak(b);
        // SAFETY: as above.
        unsafe { (self.input().on_key)(self.me(), Some(tramp), p as *mut c_void) != 0 }
    }

    /// Input capture (input group version 2). False when unsupported.
    pub fn capture_input(&self, on: bool) -> bool {
        if !has!(self.raw.input, ShkInputApi, capture) {
            return false;
        }
        // SAFETY: plain values.
        unsafe { (self.input().capture)(self.me(), on as i32) != 0 }
    }

    pub fn can_capture_input(&self) -> bool {
        has!(self.raw.input, ShkInputApi, capture)
    }

    // ---- drawing ------------------------------------------------------------------------------

    /// True when the host offers drawing primitives (`host->draw`).
    pub fn can_draw(&self) -> bool {
        has!(self.raw.draw, ShkDrawApi, on_draw)
    }

    /// Draws once per frame on `layer` (0 HUD, 1 menu, 2 notices). False when unsupported.
    pub fn on_draw(&self, layer: u32, f: impl FnMut(&mut crate::draw::HostCanvas) + Send + 'static) -> bool {
        if !self.can_draw() {
            return false;
        }
        unsafe extern "C" fn tramp(user: *mut c_void, cv: *const ShkCanvas) {
            if rt::unloading() || cv.is_null() {
                return;
            }
            let f = &mut *(user as *mut Box<dyn FnMut(&mut crate::draw::HostCanvas) + Send>);
            let mut hc = crate::draw::HostCanvas::new(&*cv);
            rt::guard("draw callback", || f(&mut hc));
        }
        let b: Box<dyn FnMut(&mut crate::draw::HostCanvas) + Send> = Box::new(f);
        let p = rt::leak(b);
        // SAFETY: as above.
        unsafe { ((*self.raw.draw).on_draw)(self.me(), layer, Some(tramp), p as *mut c_void) != 0 }
    }

    // ---- scripts ------------------------------------------------------------------------------

    pub fn has_scripts(&self) -> bool {
        has!(self.raw.scripts, ShkScriptApi, status)
    }

    /// Starts a script function on the game's script thread (its own fiber). Returning ends it.
    pub fn start_script(&self, name: &str, f: impl FnOnce() + Send + 'static) -> Option<ShkScript> {
        if !self.has_scripts() {
            return None;
        }
        unsafe extern "C" fn tramp(user: *mut c_void) {
            let slot = &mut *(user as *mut Option<Box<dyn FnOnce() + Send>>);
            if let Some(f) = slot.take() {
                rt::guard("script", f);
            }
        }
        let b: Option<Box<dyn FnOnce() + Send>> = Some(Box::new(f));
        let p = rt::leak(b);
        let n = c(name);
        // SAFETY: leaked state outlives the script.
        let id = unsafe { ((*self.raw.scripts).start)(self.me(), n.as_ptr(), Some(tramp), p as *mut c_void) };
        (id > 0).then_some(id)
    }

    pub fn stop_script(&self, id: ShkScript) -> bool {
        // SAFETY: plain values.
        self.has_scripts() && unsafe { ((*self.raw.scripts).stop)(self.me(), id) != 0 }
    }

    pub fn script_status(&self) -> ScriptStatus {
        let mut s = ShkScriptStatus::default();
        let m = if self.has_scripts() {
            // SAFETY: out parameter.
            unsafe { ((*self.raw.scripts).status)(&mut s) as u32 }
        } else {
            0
        };
        ScriptStatus {
            mode: match m {
                SHK_SCRIPTS_GAME_THREAD => ScriptMode::GameThread,
                SHK_SCRIPTS_PRESENT_FALLBACK => ScriptMode::PresentFallback,
                _ => ScriptMode::Off,
            },
            scripts: s.scripts,
            ticks: s.ticks,
            tick_thread: s.tick_thread,
            in_script: s.in_script != 0,
            reason: fixed_str(&s.reason),
        }
    }

    // ---- natives ------------------------------------------------------------------------------

    pub fn has_natives(&self) -> bool {
        has!(self.raw.natives, ShkNativeApi, hash)
    }

    pub(crate) fn natives_api(&self) -> Option<&'static ShkNativeApi> {
        // SAFETY: checked presence.
        self.has_natives().then(|| unsafe { &*self.raw.natives })
    }

    /// The engine's name hash.
    pub fn hash(&self, s: &str) -> u32 {
        crate::hash::joaat(s)
    }

    // ---- host basic menu ----------------------------------------------------------------------

    pub fn basic_menu(&self) -> Option<&'static ShkHostMenuApi> {
        // SAFETY: checked presence.
        has!(self.raw.menu, ShkHostMenuApi, set_text).then(|| unsafe { &*self.raw.menu })
    }

    // ---- services -----------------------------------------------------------------------------

    pub fn has_services(&self) -> bool {
        has!(self.raw.services, ShkServiceApi, find)
    }

    /// Publishes `iface` under `name` (it must stay valid until withdrawn / unload).
    pub fn publish(&self, name: &str, iface: *const c_void) -> bool {
        let n = c(name);
        // SAFETY: valid pointers.
        self.has_services() && unsafe { ((*self.raw.services).publish)(self.me(), n.as_ptr(), iface) != 0 }
    }
    pub fn withdraw(&self, name: &str) -> bool {
        let n = c(name);
        // SAFETY: valid pointers.
        self.has_services() && unsafe { ((*self.raw.services).withdraw)(self.me(), n.as_ptr()) != 0 }
    }
    pub fn find_service(&self, name: &str) -> Option<*const c_void> {
        if !self.has_services() {
            return None;
        }
        let n = c(name);
        // SAFETY: valid string.
        let p = unsafe { ((*self.raw.services).find)(n.as_ptr()) };
        (!p.is_null()).then_some(p)
    }
}

unsafe extern "C" fn event_tramp(user: *mut c_void) {
    if rt::unloading() {
        return;
    }
    let f = &mut *(user as *mut Box<dyn FnMut() + Send>);
    rt::guard("event callback", || f());
}

fn event_box(f: impl FnMut() + Send + 'static) -> *mut c_void {
    let b: Box<dyn FnMut() + Send> = Box::new(f);
    rt::leak(b) as *mut c_void
}

/// Yields the current script function for at least `ms` milliseconds (0 = next tick). False
/// outside a script function.
pub fn wait(ms: u32) -> bool {
    let h = crate::host();
    // SAFETY: checked presence.
    h.has_scripts() && unsafe { ((*h.raw.scripts).wait)(ms) != 0 }
}
