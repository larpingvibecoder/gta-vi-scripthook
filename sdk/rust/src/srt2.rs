//! Adapter for hosts speaking Sonoma Runtime's plugin API version 2 (`srt_plugin_init`): builds
//! an `ShkHost` from the version-2 function table. The Rust twin of `sdk/c/scripthook_srt2.c`
//! (see there and docs/host-integration.md). [`crate::export_plugin!`] exports both entry points.

use crate::ffi::*;
use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

/// Member indices of the version-2 table (after `size` and `version`).
mod idx {
    pub const LOG: usize = 0;
    pub const NOTIFY: usize = 1;
    pub const TITLE_IDENTITY: usize = 2;
    pub const MOD_FOLDER: usize = 3;
    pub const READ: usize = 4;
    pub const WRITE: usize = 5;
    pub const PATCH: usize = 6;
    pub const QUERY: usize = 7;
    pub const MODULE_INFO: usize = 8;
    pub const ENUM_MODULES: usize = 9;
    pub const FIND_EXPORT: usize = 10;
    pub const SECTION: usize = 11;
    pub const PATTERN_SCAN: usize = 12;
    pub const PATTERN_SCAN_ALL: usize = 13;
    pub const RIP_TARGET: usize = 14;
    pub const RESOLVE_CHAIN: usize = 15;
    pub const HOOK: usize = 16;
    pub const UNHOOK: usize = 17;
    pub const ON_FRAME: usize = 18;
    pub const REGISTER_HOTKEY: usize = 19;
    pub const REGISTER_PAD_CHORD: usize = 20;
    pub const KEY_DOWN: usize = 21;
    pub const PAD_STATE: usize = 22;
    pub const MENU_PAGE: usize = 23;
    pub const MENU_TOGGLE: usize = 24;
    pub const MENU_SLIDER: usize = 25;
    pub const MENU_NUMBER: usize = 26;
    pub const MENU_BUTTON: usize = 27;
    pub const MENU_TEXT: usize = 28;
    pub const MENU_SET_TEXT: usize = 29;
    pub const NATIVE_LOOKUP: usize = 30;
    pub const NATIVE_CALL: usize = 31;
    pub const HASH_KEY: usize = 32;
    pub const SCRIPT_REGISTER: usize = 33;
    pub const SCRIPT_UNREGISTER: usize = 34;
    pub const SCRIPT_WAIT: usize = 35;
    pub const SCRIPT_STATUS: usize = 36;
    pub const COUNT_V1: usize = 30;
    pub const COUNT_V2: usize = 37;
}

struct Tables {
    core: ShkCoreApi,
    memory: ShkMemoryApi,
    hooks: ShkHookApi,
    input: ShkInputApi,
    natives: ShkNativeApi,
    scripts: ShkScriptApi,
    menu: ShkHostMenuApi,
    services: ShkServiceApi,
    host: ShkHost,
    _version: CString,
}
// SAFETY: immutable function tables and 'static strings.
unsafe impl Sync for Tables {}
unsafe impl Send for Tables {}

static TABLES: OnceLock<Box<Tables>> = OnceLock::new();

unsafe extern "C" fn no_key(_: *mut ShkPlugin, _: Option<ShkKeyFn>, _: *mut c_void) -> i32 {
    0
}
unsafe extern "C" fn no_capture(_: *mut ShkPlugin, _: i32) -> i32 {
    0
}

/// Builds the host from a version-2 table (`api` = `const SrtApi*`). None if it is too old.
///
/// # Safety
/// `api` must point at a Sonoma Runtime plugin API table.
pub unsafe fn host_from_v2(api: *const c_void) -> Option<*const ShkHost> {
    if api.is_null() {
        return None;
    }
    let size = *(api as *const u32);
    let version = *(api as *const u32).add(1);
    let fns = (api as *const u8).add(8) as *const *const c_void;
    let count = (size as usize).saturating_sub(8) / 8;
    if count < idx::COUNT_V1 {
        return None;
    }
    let f = |i: usize| -> *const c_void { *fns.add(i) };
    macro_rules! t {
        ($i:expr) => {
            std::mem::transmute::<*const c_void, _>(f($i))
        };
    }
    let has_scripts = version >= 2 && count >= idx::COUNT_V2;
    let tables = TABLES.get_or_init(|| {
        let vtext = CString::new(format!("plugin API {version} via the scripthook adapter")).unwrap_or_default();
        let mut t = Box::new(Tables {
            core: ShkCoreApi { size: std::mem::size_of::<ShkCoreApi>() as u32, version: 1, log: t!(idx::LOG), notify: t!(idx::NOTIFY), title_identity: t!(idx::TITLE_IDENTITY), plugin_folder: t!(idx::MOD_FOLDER) },
            memory: ShkMemoryApi {
                size: std::mem::size_of::<ShkMemoryApi>() as u32,
                version: 1,
                read: t!(idx::READ),
                write: t!(idx::WRITE),
                patch: t!(idx::PATCH),
                query: t!(idx::QUERY),
                module: t!(idx::MODULE_INFO),
                modules: t!(idx::ENUM_MODULES),
                find_export: t!(idx::FIND_EXPORT),
                section: t!(idx::SECTION),
                scan: t!(idx::PATTERN_SCAN),
                scan_all: t!(idx::PATTERN_SCAN_ALL),
                rip_target: t!(idx::RIP_TARGET),
                resolve_chain: t!(idx::RESOLVE_CHAIN),
            },
            hooks: ShkHookApi { size: std::mem::size_of::<ShkHookApi>() as u32, version: 1, install: t!(idx::HOOK), remove: t!(idx::UNHOOK) },
            input: ShkInputApi {
                // version 1: up to `pad`
                size: std::mem::offset_of!(ShkInputApi, on_key) as u32,
                version: 1,
                on_frame: t!(idx::ON_FRAME),
                hotkey: t!(idx::REGISTER_HOTKEY),
                pad_chord: t!(idx::REGISTER_PAD_CHORD),
                key_down: t!(idx::KEY_DOWN),
                pad: t!(idx::PAD_STATE),
                on_key: no_key,
                capture: no_capture,
            },
            natives: if has_scripts {
                ShkNativeApi { size: std::mem::size_of::<ShkNativeApi>() as u32, version: 1, lookup: t!(idx::NATIVE_LOOKUP), call: t!(idx::NATIVE_CALL), hash: t!(idx::HASH_KEY) }
            } else {
                ShkNativeApi { size: 0, version: 0, lookup: none_lookup, call: none_call, hash: crate::hash::joaat_c }
            },
            scripts: if has_scripts {
                ShkScriptApi { size: std::mem::size_of::<ShkScriptApi>() as u32, version: 1, start: t!(idx::SCRIPT_REGISTER), stop: t!(idx::SCRIPT_UNREGISTER), wait: t!(idx::SCRIPT_WAIT), status: t!(idx::SCRIPT_STATUS) }
            } else {
                ShkScriptApi { size: 0, version: 0, start: none_start, stop: none_stop, wait: none_wait, status: none_status }
            },
            menu: ShkHostMenuApi {
                size: std::mem::size_of::<ShkHostMenuApi>() as u32,
                version: 1,
                page: t!(idx::MENU_PAGE),
                toggle: t!(idx::MENU_TOGGLE),
                slider: t!(idx::MENU_SLIDER),
                number: t!(idx::MENU_NUMBER),
                button: t!(idx::MENU_BUTTON),
                text: t!(idx::MENU_TEXT),
                set_text: t!(idx::MENU_SET_TEXT),
            },
            services: ShkServiceApi { size: std::mem::size_of::<ShkServiceApi>() as u32, version: 1, publish: crate::services::publish, withdraw: crate::services::withdraw, find: crate::services::find },
            host: ShkHost {
                size: std::mem::size_of::<ShkHost>() as u32,
                abi_version: SHK_ABI_VERSION,
                host_name: c"Sonoma Runtime".as_ptr(),
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
            _version: vtext,
        });
        let p: *mut Tables = &mut *t;
        let tt = &mut *p;
        tt.host.host_version = tt._version.as_ptr();
        tt.host.core = &tt.core;
        tt.host.memory = &tt.memory;
        tt.host.hooks = &tt.hooks;
        tt.host.input = &tt.input;
        if has_scripts {
            tt.host.natives = &tt.natives;
            tt.host.scripts = &tt.scripts;
        }
        tt.host.menu = &tt.menu;
        tt.host.services = &tt.services;
        t
    });
    Some(&tables.host)
}

unsafe extern "C" fn none_lookup(_: *const c_char, _: *mut ShkNativeInfo) -> ShkNative {
    0
}
unsafe extern "C" fn none_call(_: ShkNative, _: *const ShkValue, _: u32, _: *mut ShkValue) -> i32 {
    0
}
unsafe extern "C" fn none_start(_: *mut ShkPlugin, _: *const c_char, _: Option<ShkScriptFn>, _: *mut c_void) -> ShkScript {
    0
}
unsafe extern "C" fn none_stop(_: *mut ShkPlugin, _: ShkScript) -> i32 {
    0
}
unsafe extern "C" fn none_wait(_: u32) -> i32 {
    0
}
unsafe extern "C" fn none_status(_: *mut ShkScriptStatus) -> i32 {
    0
}

/// `srt_plugin_init` body: adapts and calls the plugin's `shk_plugin_load`.
///
/// # Safety
/// Called by the host with its table.
pub unsafe fn init(api: *const c_void, me: *mut ShkPlugin, load: unsafe extern "C" fn(*const ShkHost, *mut ShkPlugin) -> i32) -> i32 {
    match host_from_v2(api) {
        Some(h) => {
            let r = load(h, me);
            if r != 0 {
                crate::services::close(me);
            }
            r
        }
        None => 1,
    }
}

/// `srt_plugin_shutdown` body.
///
/// # Safety
/// Called by the host once, at unload.
pub unsafe fn shutdown(unload: unsafe extern "C" fn()) {
    unload();
    crate::services::close(crate::rt::self_ptr());
}
