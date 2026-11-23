//! The `scripthook.h` host the hook serves to plugins: every group implemented by the hook's own
//! code (memory, patterns, hooks, input, natives, script fibers, drawing, services).

use crate::overlay::DrawList;
use crate::{hook, input, locate, log, mem, natives::Natives, scripts};
use scripthook_sdk::ffi::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

/// What `ShkPlugin*` points at.
pub struct PluginCtx {
    pub id: u64,
    pub name: String,
    pub folder: CString,
}

unsafe fn ctx<'a>(p: *mut ShkPlugin) -> Option<&'a PluginCtx> {
    (p as *const PluginCtx).as_ref()
}

unsafe fn owner(p: *mut ShkPlugin) -> u64 {
    ctx(p).map(|c| c.id).unwrap_or(0)
}

unsafe fn s(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

#[derive(Clone, Copy)]
struct Cb {
    owner: u64,
    f: usize,
    user: usize,
    a: u32,
    b: u32,
}

#[derive(Default)]
struct Reg {
    frames: Vec<Cb>,
    hotkeys: Vec<Cb>,
    chords: Vec<Cb>,
    keys: Vec<Cb>,
    draws: Vec<Cb>,
    services: Vec<(String, u64, usize)>,
    captures: Vec<u64>,
}

static REG: Mutex<Option<Reg>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Reg) -> R) -> R {
    let mut g = REG.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Reg::default))
}

pub static NATIVES: Mutex<Option<Natives>> = Mutex::new(None);
static IDENTITY: OnceLock<CString> = OnceLock::new();
static NOTICES: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());

pub fn set_identity(s: &str) {
    let _ = IDENTITY.set(CString::new(s).unwrap_or_default());
}

// ---- core ----------------------------------------------------------------------------------

unsafe extern "C" fn core_log(p: *mut ShkPlugin, level: i32, t: *const c_char) {
    let m = format!("{}: {}", ctx(p).map(|c| c.name.as_str()).unwrap_or("plugin"), s(t));
    match level {
        0 => log::error(&m),
        1 => log::warn(&m),
        3 => log::debug(&m),
        _ => log::info(&m),
    }
}

pub fn notify(text: &str, secs: f32) {
    log::info(&format!("notice: {text}"));
    let mut n = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
    n.push((text.to_string(), shk_sys::now() + secs.clamp(0.5, 30.0) as f64));
    if n.len() > 4 {
        n.remove(0);
    }
}

unsafe extern "C" fn core_notify(_p: *mut ShkPlugin, t: *const c_char, secs: f32) {
    notify(&s(t), secs);
}
unsafe extern "C" fn core_identity() -> *const c_char {
    IDENTITY.get().map(|c| c.as_ptr()).unwrap_or(c"".as_ptr())
}
unsafe extern "C" fn core_folder(p: *mut ShkPlugin) -> *const c_char {
    ctx(p).map(|c| c.folder.as_ptr()).unwrap_or(c"".as_ptr())
}

// ---- memory --------------------------------------------------------------------------------

unsafe extern "C" fn m_read(a: u64, out: *mut c_void, n: usize) -> i32 {
    (!out.is_null() && mem::read(a as usize, std::slice::from_raw_parts_mut(out as *mut u8, n))) as i32
}
unsafe extern "C" fn m_write(a: u64, d: *const c_void, n: usize) -> i32 {
    (!d.is_null() && mem::write(a as usize, std::slice::from_raw_parts(d as *const u8, n))) as i32
}
unsafe extern "C" fn m_patch(a: u64, d: *const c_void, n: usize) -> i32 {
    (!d.is_null() && mem::patch(a as usize, std::slice::from_raw_parts(d as *const u8, n))) as i32
}
unsafe extern "C" fn m_query(a: u64, out: *mut ShkRegion) -> i32 {
    let Some(m) = mem::query(a as usize) else { return 0 };
    if !out.is_null() {
        *out = ShkRegion { base: m.BaseAddress as u64, size: m.RegionSize as u64, protect: m.Protect, state: m.State, kind: m.Type, readable: (m.State == shk_sys::MEM_COMMIT && mem::readable(m.Protect)) as u32, writable: (m.State == shk_sys::MEM_COMMIT && mem::writable(m.Protect)) as u32, reserved: 0 };
    }
    1
}
fn fill(m: &mem::Module, out: &mut ShkModule) {
    out.base = m.base as u64;
    out.size = m.size as u64;
    out.name = [0; 64];
    let b = m.name.as_bytes();
    let n = b.len().min(63);
    out.name[..n].copy_from_slice(&b[..n]);
    out.is_title = (m.base == locate::module("").map(|g| g.base).unwrap_or(0)) as u32;
    out.sections = m.sections.len() as u32;
}
unsafe extern "C" fn m_module(name: *const c_char, out: *mut ShkModule) -> i32 {
    let Some(m) = locate::module(&s(name)) else { return 0 };
    if !out.is_null() {
        fill(&m, &mut *out);
    }
    1
}
unsafe extern "C" fn m_modules(f: Option<ShkModuleFn>, user: *mut c_void) -> i32 {
    let ms = locate::modules();
    if let Some(f) = f {
        for m in &ms {
            let mut x: ShkModule = std::mem::zeroed();
            fill(m, &mut x);
            f(user, &x);
        }
    }
    ms.len() as i32
}
unsafe extern "C" fn m_export(m: *const c_char, sym: *const c_char) -> u64 {
    locate::module(&s(m)).and_then(|m| m.export(&s(sym))).unwrap_or(0) as u64
}
unsafe extern "C" fn m_section(m: *const c_char, n: *const c_char, start: *mut u64, size: *mut u64) -> i32 {
    let Some(m) = locate::module(&s(m)) else { return 0 };
    let Some(sec) = m.section(&s(n)) else { return 0 };
    if !start.is_null() {
        *start = sec.start as u64;
    }
    if !size.is_null() {
        *size = sec.size as u64;
    }
    1
}
unsafe fn scan(m: *const c_char, sec: *const c_char, pat: *const c_char, max: usize) -> Vec<usize> {
    let Some(m) = locate::module(&s(m)) else { return Vec::new() };
    let Ok(p) = mem::parse_pattern(&s(pat)) else { return Vec::new() };
    let sec = if sec.is_null() { None } else { Some(s(sec)) };
    mem::scan_module(&m, sec.as_deref(), &p, max)
}
unsafe extern "C" fn m_scan(m: *const c_char, sec: *const c_char, pat: *const c_char, nth: u32) -> u64 {
    scan(m, sec, pat, nth as usize + 1).get(nth as usize).copied().unwrap_or(0) as u64
}
unsafe extern "C" fn m_scan_all(m: *const c_char, sec: *const c_char, pat: *const c_char, out: *mut u64, max: usize) -> usize {
    let v = scan(m, sec, pat, max.max(1));
    if !out.is_null() {
        for (k, a) in v.iter().take(max).enumerate() {
            *out.add(k) = *a as u64;
        }
    }
    v.len().min(max)
}
unsafe extern "C" fn m_rip(i: u64, d: u32, l: u32) -> u64 {
    mem::rip_target(i as usize, d as usize, l as usize).unwrap_or(0) as u64
}
unsafe extern "C" fn m_chain(b: u64, o: *const i64, n: usize) -> u64 {
    let offs = if o.is_null() || n == 0 { &[][..] } else { std::slice::from_raw_parts(o, n) };
    mem::resolve_chain(b as usize, offs).unwrap_or(0) as u64
}

// ---- hooks ---------------------------------------------------------------------------------

unsafe extern "C" fn h_install(p: *mut ShkPlugin, target: u64, detour: *mut c_void, original: *mut *mut c_void) -> u64 {
    match hook::install(target as usize, detour as usize, owner(p)) {
        Ok((id, t)) => {
            if !original.is_null() {
                *original = t as *mut c_void;
            }
            id
        }
        Err(e) => {
            log::warn(&format!("{}: hook of {target:#x} failed: {e}", ctx(p).map(|c| c.name.as_str()).unwrap_or("plugin")));
            0
        }
    }
}
unsafe extern "C" fn h_remove(_p: *mut ShkPlugin, h: u64) -> i32 {
    hook::remove(h).is_ok() as i32
}

// ---- input ---------------------------------------------------------------------------------

unsafe extern "C" fn i_frame(p: *mut ShkPlugin, f: Option<ShkFrameFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    with(|r| r.frames.push(Cb { owner: owner(p), f: f as usize, user: user as usize, a: 0, b: 0 }));
    1
}
unsafe extern "C" fn i_hotkey(p: *mut ShkPlugin, vk: u32, mods: u32, f: Option<ShkEventFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    with(|r| r.hotkeys.push(Cb { owner: owner(p), f: f as usize, user: user as usize, a: vk, b: mods }));
    1
}
unsafe extern "C" fn i_chord(p: *mut ShkPlugin, buttons: u32, f: Option<ShkEventFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    if buttons == 0 {
        return 0;
    }
    with(|r| r.chords.push(Cb { owner: owner(p), f: f as usize, user: user as usize, a: buttons, b: 0 }));
    1
}
unsafe extern "C" fn i_key_down(vk: u32) -> i32 {
    input::key_held(vk) as i32
}
unsafe extern "C" fn i_pad(user: u32, out: *mut ShkPad) -> i32 {
    match input::pad(user & 3) {
        Some(p) => {
            if !out.is_null() {
                *out = p;
            }
            1
        }
        None => 0,
    }
}
unsafe extern "C" fn i_on_key(p: *mut ShkPlugin, f: Option<ShkKeyFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    with(|r| r.keys.push(Cb { owner: owner(p), f: f as usize, user: user as usize, a: 0, b: 0 }));
    1
}
unsafe extern "C" fn i_capture(p: *mut ShkPlugin, on: i32) -> i32 {
    let o = owner(p);
    let any = with(|r| {
        r.captures.retain(|x| *x != o);
        if on != 0 {
            r.captures.push(o);
        }
        !r.captures.is_empty()
    });
    input::capture(any);
    1
}

// ---- natives and scripts -------------------------------------------------------------------

static HANDLES: Mutex<Vec<String>> = Mutex::new(Vec::new());

unsafe extern "C" fn n_lookup(name: *const c_char, info: *mut ShkNativeInfo) -> u64 {
    let n = s(name);
    let mut g = NATIVES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(nat) = g.as_mut() else {
        if !info.is_null() {
            *info = ShkNativeInfo::default();
            let r = b"the natives database is not loaded (natives.tsv)";
            let i = &mut *info;
            i.reason[..r.len()].copy_from_slice(r);
        }
        return 0;
    };
    let key = nat.db.find(&n).map(|x| x.name.clone()).or_else(|| n.starts_with("0x").then(|| n.clone()));
    if !info.is_null() {
        nat.info(&n, &mut *info);
    }
    let Some(k) = key else { return 0 };
    let mut hs = HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let i = hs.iter().position(|x| *x == k).unwrap_or_else(|| {
        hs.push(k);
        hs.len() - 1
    });
    i as u64 + 1
}

static CALLS: AtomicU32 = AtomicU32::new(0);
static FAILS: AtomicU32 = AtomicU32::new(0);

pub fn native_stats() -> (u32, u32) {
    (CALLS.load(Ordering::Relaxed), FAILS.load(Ordering::Relaxed))
}

unsafe extern "C" fn n_call(h: u64, args: *const ShkValue, count: u32, result: *mut ShkValue) -> i32 {
    let Some(name) = HANDLES.lock().unwrap_or_else(|e| e.into_inner()).get((h as usize).wrapping_sub(1)).cloned() else { return 0 };
    let a = if args.is_null() || count == 0 { &[][..] } else { std::slice::from_raw_parts(args, count.min(64) as usize) };
    let want = if result.is_null() { SHK_TYPE_VOID } else { (*result).ty };
    CALLS.fetch_add(1, Ordering::Relaxed);
    let r = {
        let mut g = NATIVES.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_mut() {
            Some(n) => n.call(&name, a, want),
            None => Err("the natives database is not loaded".into()),
        }
    };
    match r {
        Ok(v) => {
            if !result.is_null() {
                *result = v;
            }
            1
        }
        Err(e) => {
            if FAILS.fetch_add(1, Ordering::Relaxed) < 50 {
                log::warn(&format!("native {name}: {e}"));
            }
            0
        }
    }
}
unsafe extern "C" fn n_hash(t: *const c_char) -> u32 {
    if t.is_null() {
        0
    } else {
        scripthook_sdk::hash::joaat_bytes(CStr::from_ptr(t).to_bytes())
    }
}

unsafe extern "C" fn s_start(p: *mut ShkPlugin, name: *const c_char, f: Option<ShkScriptFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    let n = format!("{}: {}", ctx(p).map(|c| c.name.as_str()).unwrap_or("plugin"), s(name));
    scripts::register(owner(p), &n, f, user as usize) as i32
}
unsafe extern "C" fn s_stop(p: *mut ShkPlugin, id: i32) -> i32 {
    scripts::unregister(owner(p), id as u32) as i32
}
unsafe extern "C" fn s_wait(ms: u32) -> i32 {
    scripts::wait(ms) as i32
}
unsafe extern "C" fn s_status(out: *mut ShkScriptStatus) -> i32 {
    let m = scripts::mode();
    if !out.is_null() {
        let o = &mut *out;
        *o = ShkScriptStatus::default();
        o.mode = m as u32;
        o.scripts = scripts::count();
        o.ticks = scripts::ticks();
        o.tick_thread = scripts::tick_thread();
        o.in_script = scripts::in_script() as u32;
        let r = scripts::reason();
        let b = r.as_bytes();
        let k = b.len().min(159);
        o.reason[..k].copy_from_slice(&b[..k]);
    }
    m as i32
}

// ---- drawing -------------------------------------------------------------------------------

unsafe extern "C" fn d_on_draw(p: *mut ShkPlugin, layer: u32, f: Option<ShkDrawFn>, user: *mut c_void) -> i32 {
    let Some(f) = f else { return 0 };
    with(|r| r.draws.push(Cb { owner: owner(p), f: f as usize, user: user as usize, a: layer.min(2), b: 0 }));
    1
}

unsafe extern "C" fn c_rect(ctx: *mut c_void, x: f32, y: f32, w: f32, h: f32, c: u32) {
    (*(ctx as *mut DrawList)).rect(x, y, w, h, c)
}
unsafe extern "C" fn c_text(ctx: *mut c_void, x: f32, y: f32, scale: f32, c: u32, t: *const c_char) -> f32 {
    (*(ctx as *mut DrawList)).text(x, y, scale, c, &s(t))
}
unsafe extern "C" fn c_clip(ctx: *mut c_void, x: f32, y: f32, w: f32, h: f32) {
    (*(ctx as *mut DrawList)).set_clip(x, y, w, h)
}
unsafe extern "C" fn c_measure(_ctx: *mut c_void, scale: f32, t: *const c_char) -> f32 {
    scripthook_sdk::font::atlas().text_width(&s(t), scale)
}

// ---- services ------------------------------------------------------------------------------

unsafe extern "C" fn sv_publish(p: *mut ShkPlugin, name: *const c_char, iface: *const c_void) -> i32 {
    let (n, o) = (s(name), owner(p));
    if n.is_empty() || iface.is_null() {
        return 0;
    }
    with(|r| {
        if r.services.iter().any(|x| x.0 == n && x.1 != o) {
            return 0;
        }
        r.services.retain(|x| x.0 != n);
        r.services.push((n, o, iface as usize));
        1
    })
}
unsafe extern "C" fn sv_withdraw(p: *mut ShkPlugin, name: *const c_char) -> i32 {
    let (n, o) = (s(name), owner(p));
    with(|r| {
        let before = r.services.len();
        r.services.retain(|x| !(x.0 == n && x.1 == o));
        (before != r.services.len()) as i32
    })
}
unsafe extern "C" fn sv_find(name: *const c_char) -> *const c_void {
    let n = s(name);
    with(|r| r.services.iter().find(|x| x.0 == n).map(|x| x.2 as *const c_void).unwrap_or(std::ptr::null()))
}

// ---- the tables ----------------------------------------------------------------------------

struct Tables {
    core: ShkCoreApi,
    memory: ShkMemoryApi,
    hooks: ShkHookApi,
    input: ShkInputApi,
    natives: ShkNativeApi,
    scripts: ShkScriptApi,
    draw: ShkDrawApi,
    services: ShkServiceApi,
    host: ShkHost,
    version: CString,
}
// SAFETY: immutable after construction.
unsafe impl Sync for Tables {}
unsafe impl Send for Tables {}

static TABLES: OnceLock<Box<Tables>> = OnceLock::new();

/// The host table handed to plugins.
pub fn host() -> *const ShkHost {
    let t = TABLES.get_or_init(|| {
        let mut t = Box::new(Tables {
            core: ShkCoreApi { size: std::mem::size_of::<ShkCoreApi>() as u32, version: 1, log: core_log, notify: core_notify, title_identity: core_identity, plugin_folder: core_folder },
            memory: ShkMemoryApi { size: std::mem::size_of::<ShkMemoryApi>() as u32, version: 1, read: m_read, write: m_write, patch: m_patch, query: m_query, module: m_module, modules: m_modules, find_export: m_export, section: m_section, scan: m_scan, scan_all: m_scan_all, rip_target: m_rip, resolve_chain: m_chain },
            hooks: ShkHookApi { size: std::mem::size_of::<ShkHookApi>() as u32, version: 1, install: h_install, remove: h_remove },
            input: ShkInputApi { size: std::mem::size_of::<ShkInputApi>() as u32, version: 2, on_frame: i_frame, hotkey: i_hotkey, pad_chord: i_chord, key_down: i_key_down, pad: i_pad, on_key: i_on_key, capture: i_capture },
            natives: ShkNativeApi { size: std::mem::size_of::<ShkNativeApi>() as u32, version: 1, lookup: n_lookup, call: n_call, hash: n_hash },
            scripts: ShkScriptApi { size: std::mem::size_of::<ShkScriptApi>() as u32, version: 1, start: s_start, stop: s_stop, wait: s_wait, status: s_status },
            draw: ShkDrawApi { size: std::mem::size_of::<ShkDrawApi>() as u32, version: 1, on_draw: d_on_draw },
            services: ShkServiceApi { size: std::mem::size_of::<ShkServiceApi>() as u32, version: 1, publish: sv_publish, withdraw: sv_withdraw, find: sv_find },
            host: ShkHost {
                size: std::mem::size_of::<ShkHost>() as u32,
                abi_version: SHK_ABI_VERSION,
                host_name: c"gta-vi-scripthook".as_ptr(),
                host_version: std::ptr::null(),
                core: std::ptr::null(),
                memory: std::ptr::null(),
                hooks: std::ptr::null(),
                input: std::ptr::null(),
                natives: std::ptr::null(),
                scripts: std::ptr::null(),
                menu: std::ptr::null(),
                draw: std::ptr::null(),
                services: std::ptr::null(),
            },
            version: CString::new(env!("CARGO_PKG_VERSION")).unwrap(),
        });
        let p: *mut Tables = &mut *t;
        // SAFETY: pointers into the boxed tables, which live for the process.
        unsafe {
            let tt = &mut *p;
            tt.host.host_version = tt.version.as_ptr();
            tt.host.core = &tt.core;
            tt.host.memory = &tt.memory;
            tt.host.hooks = &tt.hooks;
            tt.host.input = &tt.input;
            tt.host.natives = &tt.natives;
            tt.host.scripts = &tt.scripts;
            tt.host.draw = &tt.draw;
            tt.host.services = &tt.services;
        }
        t
    });
    &t.host
}

/// Everything `owner` registered.
pub fn remove_owner(owner: u64) {
    scripts::remove_owner(owner);
    hook::remove_owner(owner);
    with(|r| {
        r.frames.retain(|c| c.owner != owner);
        r.hotkeys.retain(|c| c.owner != owner);
        r.chords.retain(|c| c.owner != owner);
        r.keys.retain(|c| c.owner != owner);
        r.draws.retain(|c| c.owner != owner);
        r.services.retain(|c| c.1 != owner);
        r.captures.retain(|c| *c != owner);
    });
}

// ---- the frame -----------------------------------------------------------------------------

pub struct FrameState {
    pub poll: input::Poll,
    pub last: f64,
    pub frame: u64,
    pub pad_prev: u32,
}

static FRAME: Mutex<Option<FrameState>> = Mutex::new(None);

/// Every present: input, frame callbacks, the present-time script fallback, drawing.
pub fn frame(dl: &mut DrawList, w: u32, h: u32) {
    let now = shk_sys::now();
    let (events, info, chords) = {
        let mut g = FRAME.lock().unwrap_or_else(|e| e.into_inner());
        let st = g.get_or_insert_with(|| FrameState { poll: input::Poll::default(), last: now, frame: 0, pad_prev: 0 });
        let ev = st.poll.keyboard(now);
        let dt = (now - st.last).clamp(0.0, 0.25);
        st.last = now;
        st.frame += 1;
        let held = input::pad(0).map(|p| p.buttons).unwrap_or(0);
        let rising = held & !st.pad_prev;
        st.pad_prev = held;
        let captured = input::CAPTURED.load(Ordering::Relaxed);
        // menu_open: the hook has no menu of its own (a plugin's open menu captures input instead)
        let _ = captured;
        (ev, ShkFrameInfo { frame: st.frame, dt, width: w, height: h, menu_open: 0, reserved: 0 }, (held, rising))
    };
    let (keys, hotkeys, frames, chordcbs) = with(|r| (r.keys.clone(), r.hotkeys.clone(), r.frames.clone(), r.chords.clone()));
    let captured = input::CAPTURED.load(Ordering::Relaxed);
    let mods = input::key_held(0x10) as u32 | (input::key_held(0x11) as u32) << 1 | (input::key_held(0x12) as u32) << 2;
    for e in &events {
        let mut consumed = false;
        for k in &keys {
            // SAFETY: the plugin's registered callback.
            let f: ShkKeyFn = unsafe { std::mem::transmute(k.f) };
            if unsafe { f(k.user as *mut c_void, e) } != 0 {
                consumed = true;
                break;
            }
        }
        if !consumed && !captured && e.down == 1 && e.repeat == 0 && e.vk != 0 {
            for hk in hotkeys.iter().filter(|h| h.a == e.vk && h.b == mods) {
                // SAFETY: as above.
                unsafe { std::mem::transmute::<usize, ShkEventFn>(hk.f)(hk.user as *mut c_void) };
            }
        }
    }
    if !captured {
        for c in chordcbs.iter().filter(|c| chords.0 & c.a == c.a && chords.1 & c.a != 0) {
            // SAFETY: as above.
            unsafe { std::mem::transmute::<usize, ShkEventFn>(c.f)(c.user as *mut c_void) };
        }
    }
    for f in &frames {
        // SAFETY: as above.
        unsafe { std::mem::transmute::<usize, ShkFrameFn>(f.f)(f.user as *mut c_void, &info) };
    }
    scripts::tick(scripts::Mode::PresentFallback);
    // drawing: HUD layer, menu layer, notices layer
    let at = scripthook_sdk::font::atlas();
    let scale = scripthook_sdk::draw::scale_for_height(h);
    let canvas = ShkCanvas {
        size: std::mem::size_of::<ShkCanvas>() as u32,
        width: w,
        height: h,
        ui_scale: scale,
        cap_height: at.face(scripthook_sdk::font::px_for_scale(1.0)).cap,
        line_height: 13.0,
        reserved: [0; 2],
        ctx: dl as *mut DrawList as *mut c_void,
        rect: c_rect,
        text: c_text,
        clip: c_clip,
        measure: c_measure,
    };
    let draws = with(|r| r.draws.clone());
    for layer in 0..3 {
        for d in draws.iter().filter(|d| d.a == layer) {
            // SAFETY: the plugin's registered callback with a canvas valid during the call.
            unsafe { std::mem::transmute::<usize, ShkDrawFn>(d.f)(d.user as *mut c_void, &canvas) };
            dl.set_clip(0.0, 0.0, 0.0, 0.0);
        }
        if layer == 2 {
            draw_notices(dl, scale, h, now);
        }
    }
}

fn draw_notices(dl: &mut DrawList, s: f32, h: u32, now: f64) {
    let mut n = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
    n.retain(|x| x.1 > now);
    let cap = scripthook_sdk::font::atlas().face(scripthook_sdk::font::px_for_scale(s)).cap;
    let mut y = h as f32 - 16.0 * s;
    for (t, until) in n.iter().rev() {
        let a = ((until - now).clamp(0.0, 0.5) * 2.0) as f32;
        let fade = |c: u32| (((c >> 24) as f32 * a) as u32) << 24 | (c & 0x00FF_FFFF);
        let tw = scripthook_sdk::font::atlas().text_width(t, s);
        let bh = cap + 12.0 * s;
        y -= bh;
        dl.rect(12.0 * s, y, tw + 22.0 * s, bh, fade(0xF011_171F));
        dl.rect(12.0 * s, y, 2.0 * s, bh, fade(0xFF2E_C4B6));
        dl.text(22.0 * s, y + 6.0 * s, s, fade(0xFFE8_EDF2), t);
        y -= 4.0 * s;
    }
}
