//! The C ABI of `sdk/include/scripthook.h`, declared for Rust (same names without the `Shk`
//! prefix rules: types keep their C names). `tests/abi.rs` checks sizes and member order against
//! the header.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_void};

pub const SHK_ABI_VERSION: u32 = 1;

#[repr(C)]
pub struct ShkPlugin {
    _opaque: [u8; 0],
}

pub type ShkAddr = u64;
pub type ShkHook = u64;
pub type ShkNative = u64;
pub type ShkScript = i32;

pub const SHK_LOG_ERROR: i32 = 0;
pub const SHK_LOG_WARN: i32 = 1;
pub const SHK_LOG_INFO: i32 = 2;
pub const SHK_LOG_DEBUG: i32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShkVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ShkRegion {
    pub base: ShkAddr,
    pub size: u64,
    pub protect: u32,
    pub state: u32,
    pub kind: u32,
    pub readable: u32,
    pub writable: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkModule {
    pub base: ShkAddr,
    pub size: u64,
    pub name: [u8; 64],
    pub is_title: u32,
    pub sections: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ShkFrameInfo {
    pub frame: u64,
    pub dt: f64,
    pub width: u32,
    pub height: u32,
    pub menu_open: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShkPad {
    pub buttons: u32,
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub left_x: f32,
    pub left_y: f32,
    pub right_x: f32,
    pub right_y: f32,
}

pub const SHK_PAD_MENU: u32 = 0x1;
pub const SHK_PAD_VIEW: u32 = 0x2;
pub const SHK_PAD_A: u32 = 0x4;
pub const SHK_PAD_B: u32 = 0x8;
pub const SHK_PAD_X: u32 = 0x10;
pub const SHK_PAD_Y: u32 = 0x20;
pub const SHK_PAD_UP: u32 = 0x40;
pub const SHK_PAD_DOWN: u32 = 0x80;
pub const SHK_PAD_LEFT: u32 = 0x100;
pub const SHK_PAD_RIGHT: u32 = 0x200;
pub const SHK_PAD_LB: u32 = 0x400;
pub const SHK_PAD_RB: u32 = 0x800;
pub const SHK_PAD_LS: u32 = 0x1000;
pub const SHK_PAD_RS: u32 = 0x2000;

pub const SHK_MOD_NONE: u32 = 0;
pub const SHK_MOD_SHIFT: u32 = 1;
pub const SHK_MOD_CTRL: u32 = 2;
pub const SHK_MOD_ALT: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShkKeyEvent {
    pub vk: u32,
    pub down: u32,
    pub repeat: u32,
    pub ch: u32,
}

pub type ShkFrameFn = unsafe extern "C" fn(*mut c_void, *const ShkFrameInfo);
pub type ShkEventFn = unsafe extern "C" fn(*mut c_void);
pub type ShkModuleFn = unsafe extern "C" fn(*mut c_void, *const ShkModule);
pub type ShkScriptFn = unsafe extern "C" fn(*mut c_void);
pub type ShkKeyFn = unsafe extern "C" fn(*mut c_void, *const ShkKeyEvent) -> i32;
pub type ShkChangedFn = unsafe extern "C" fn(*mut c_void, *const c_void);

// ---- natives -----------------------------------------------------------------------------------

pub const SHK_TYPE_VOID: u32 = 0;
pub const SHK_TYPE_INT: u32 = 1;
pub const SHK_TYPE_FLOAT: u32 = 2;
pub const SHK_TYPE_BOOL: u32 = 3;
pub const SHK_TYPE_HASH: u32 = 4;
pub const SHK_TYPE_STRING: u32 = 5;
pub const SHK_TYPE_VEC3: u32 = 6;
pub const SHK_TYPE_U64: u32 = 7;
pub const SHK_TYPE_PTR: u32 = 8;
pub const SHK_TYPE_OUT_INT: u32 = 9;
pub const SHK_TYPE_OUT_FLOAT: u32 = 10;
pub const SHK_TYPE_OUT_BOOL: u32 = 11;
pub const SHK_TYPE_OUT_VEC3: u32 = 12;
pub const SHK_TYPE_UNKNOWN: u32 = 0xFFFF_FFFF;

/// `ShkValue`: 24 bytes; `u` is the 16-byte union.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkValue {
    pub ty: u32,
    pub reserved: u32,
    pub u: [u64; 2],
}

impl Default for ShkValue {
    fn default() -> Self {
        ShkValue { ty: 0, reserved: 0, u: [0, 0] }
    }
}

impl std::fmt::Debug for ShkValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShkValue {{ type: {}, u: [{:#x}, {:#x}] }}", self.ty, self.u[0], self.u[1])
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkNativeInfo {
    pub id: u64,
    pub available: u32,
    pub arg_count: u32,
    pub arg_types: [u32; 32],
    pub ret_type: u32,
    pub reserved: u32,
    pub name: [u8; 96],
    pub reason: [u8; 160],
}

impl Default for ShkNativeInfo {
    fn default() -> Self {
        // SAFETY: plain integers and byte arrays.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
pub struct ShkNativeApi {
    pub size: u32,
    pub version: u32,
    pub lookup: unsafe extern "C" fn(*const c_char, *mut ShkNativeInfo) -> ShkNative,
    pub call: unsafe extern "C" fn(ShkNative, *const ShkValue, u32, *mut ShkValue) -> i32,
    pub hash: unsafe extern "C" fn(*const c_char) -> u32,
}

// ---- scripts -----------------------------------------------------------------------------------

pub const SHK_SCRIPTS_OFF: u32 = 0;
pub const SHK_SCRIPTS_GAME_THREAD: u32 = 1;
pub const SHK_SCRIPTS_PRESENT_FALLBACK: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkScriptStatus {
    pub mode: u32,
    pub scripts: u32,
    pub ticks: u64,
    pub tick_thread: u32,
    pub in_script: u32,
    pub reason: [u8; 160],
}

impl Default for ShkScriptStatus {
    fn default() -> Self {
        // SAFETY: plain integers and bytes.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
pub struct ShkScriptApi {
    pub size: u32,
    pub version: u32,
    pub start: unsafe extern "C" fn(*mut ShkPlugin, *const c_char, Option<ShkScriptFn>, *mut c_void) -> ShkScript,
    pub stop: unsafe extern "C" fn(*mut ShkPlugin, ShkScript) -> i32,
    pub wait: unsafe extern "C" fn(u32) -> i32,
    pub status: unsafe extern "C" fn(*mut ShkScriptStatus) -> i32,
}

// ---- core, memory, hooks, input ----------------------------------------------------------------

#[repr(C)]
pub struct ShkCoreApi {
    pub size: u32,
    pub version: u32,
    pub log: unsafe extern "C" fn(*mut ShkPlugin, i32, *const c_char),
    pub notify: unsafe extern "C" fn(*mut ShkPlugin, *const c_char, f32),
    pub title_identity: unsafe extern "C" fn() -> *const c_char,
    pub plugin_folder: unsafe extern "C" fn(*mut ShkPlugin) -> *const c_char,
}

#[repr(C)]
pub struct ShkMemoryApi {
    pub size: u32,
    pub version: u32,
    pub read: unsafe extern "C" fn(ShkAddr, *mut c_void, usize) -> i32,
    pub write: unsafe extern "C" fn(ShkAddr, *const c_void, usize) -> i32,
    pub patch: unsafe extern "C" fn(ShkAddr, *const c_void, usize) -> i32,
    pub query: unsafe extern "C" fn(ShkAddr, *mut ShkRegion) -> i32,
    pub module: unsafe extern "C" fn(*const c_char, *mut ShkModule) -> i32,
    pub modules: unsafe extern "C" fn(Option<ShkModuleFn>, *mut c_void) -> i32,
    pub find_export: unsafe extern "C" fn(*const c_char, *const c_char) -> ShkAddr,
    pub section: unsafe extern "C" fn(*const c_char, *const c_char, *mut ShkAddr, *mut u64) -> i32,
    pub scan: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, u32) -> ShkAddr,
    pub scan_all: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *mut ShkAddr, usize) -> usize,
    pub rip_target: unsafe extern "C" fn(ShkAddr, u32, u32) -> ShkAddr,
    pub resolve_chain: unsafe extern "C" fn(ShkAddr, *const i64, usize) -> ShkAddr,
}

#[repr(C)]
pub struct ShkHookApi {
    pub size: u32,
    pub version: u32,
    pub install: unsafe extern "C" fn(*mut ShkPlugin, ShkAddr, *mut c_void, *mut *mut c_void) -> ShkHook,
    pub remove: unsafe extern "C" fn(*mut ShkPlugin, ShkHook) -> i32,
}

#[repr(C)]
pub struct ShkInputApi {
    pub size: u32,
    pub version: u32,
    pub on_frame: unsafe extern "C" fn(*mut ShkPlugin, Option<ShkFrameFn>, *mut c_void) -> i32,
    pub hotkey: unsafe extern "C" fn(*mut ShkPlugin, u32, u32, Option<ShkEventFn>, *mut c_void) -> i32,
    pub pad_chord: unsafe extern "C" fn(*mut ShkPlugin, u32, Option<ShkEventFn>, *mut c_void) -> i32,
    pub key_down: unsafe extern "C" fn(u32) -> i32,
    pub pad: unsafe extern "C" fn(u32, *mut ShkPad) -> i32,
    // version 2
    pub on_key: unsafe extern "C" fn(*mut ShkPlugin, Option<ShkKeyFn>, *mut c_void) -> i32,
    pub capture: unsafe extern "C" fn(*mut ShkPlugin, i32) -> i32,
}

pub type ShkHostPage = u32;

#[repr(C)]
pub struct ShkHostMenuApi {
    pub size: u32,
    pub version: u32,
    pub page: unsafe extern "C" fn(*mut ShkPlugin, *const c_char) -> ShkHostPage,
    pub toggle: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, *const c_char, *mut i32, Option<ShkChangedFn>, *mut c_void) -> i32,
    pub slider: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, *const c_char, *mut f32, f32, f32, f32, Option<ShkChangedFn>, *mut c_void) -> i32,
    pub number: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, *const c_char, *mut f64, Option<ShkChangedFn>, *mut c_void) -> i32,
    pub button: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, *const c_char, Option<ShkEventFn>, *mut c_void) -> i32,
    pub text: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, *const c_char) -> i32,
    pub set_text: unsafe extern "C" fn(*mut ShkPlugin, ShkHostPage, i32, *const c_char) -> i32,
}

// ---- drawing -----------------------------------------------------------------------------------

#[repr(C)]
pub struct ShkCanvas {
    pub size: u32,
    pub width: u32,
    pub height: u32,
    pub ui_scale: f32,
    pub cap_height: f32,
    pub line_height: f32,
    pub reserved: [u32; 2],
    pub ctx: *mut c_void,
    pub rect: unsafe extern "C" fn(*mut c_void, f32, f32, f32, f32, u32),
    pub text: unsafe extern "C" fn(*mut c_void, f32, f32, f32, u32, *const c_char) -> f32,
    pub clip: unsafe extern "C" fn(*mut c_void, f32, f32, f32, f32),
    pub measure: unsafe extern "C" fn(*mut c_void, f32, *const c_char) -> f32,
}

pub type ShkDrawFn = unsafe extern "C" fn(*mut c_void, *const ShkCanvas);

#[repr(C)]
pub struct ShkDrawApi {
    pub size: u32,
    pub version: u32,
    pub on_draw: unsafe extern "C" fn(*mut ShkPlugin, u32, Option<ShkDrawFn>, *mut c_void) -> i32,
}

// ---- services ----------------------------------------------------------------------------------

#[repr(C)]
pub struct ShkServiceApi {
    pub size: u32,
    pub version: u32,
    pub publish: unsafe extern "C" fn(*mut ShkPlugin, *const c_char, *const c_void) -> i32,
    pub withdraw: unsafe extern "C" fn(*mut ShkPlugin, *const c_char) -> i32,
    pub find: unsafe extern "C" fn(*const c_char) -> *const c_void,
}

// ---- the host ----------------------------------------------------------------------------------

#[repr(C)]
pub struct ShkHost {
    pub size: u32,
    pub abi_version: u32,
    pub host_name: *const c_char,
    pub host_version: *const c_char,
    pub core: *const ShkCoreApi,
    pub memory: *const ShkMemoryApi,
    pub hooks: *const ShkHookApi,
    pub input: *const ShkInputApi,
    pub natives: *const ShkNativeApi,
    pub scripts: *const ShkScriptApi,
    pub menu: *const ShkHostMenuApi,
    pub draw: *const ShkDrawApi,
    pub services: *const ShkServiceApi,
}

#[repr(C)]
pub struct ShkPluginInfo {
    pub size: u32,
    pub abi_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
    pub author: *const c_char,
    pub description: *const c_char,
}

/// `ShkPluginInfo` in a `static` (its pointers are to `'static` C strings).
#[repr(transparent)]
pub struct StaticPluginInfo(pub ShkPluginInfo);
// SAFETY: points at immutable 'static data only.
unsafe impl Sync for StaticPluginInfo {}

// ---- the menu service --------------------------------------------------------------------------

pub const SHK_MENU_SERVICE: &str = "scripthook.menu";

pub type ShkMenuPage = u32;
pub type ShkMenuItem = u32;

pub const SHK_ITEM_TEXT: u32 = 0;
pub const SHK_ITEM_BUTTON: u32 = 1;
pub const SHK_ITEM_TOGGLE: u32 = 2;
pub const SHK_ITEM_SLIDER: u32 = 3;
pub const SHK_ITEM_NUMBER: u32 = 4;
pub const SHK_ITEM_CHOICE: u32 = 5;
pub const SHK_ITEM_INPUT: u32 = 6;
pub const SHK_ITEM_SUBPAGE: u32 = 7;
pub const SHK_ITEM_SEPARATOR: u32 = 8;

pub const SHK_ITEMF_DISABLED: u32 = 1;
pub const SHK_ITEMF_FILTERABLE: u32 = 2;

pub const SHK_EVENT_ACTIVATE: u32 = 1;
pub const SHK_EVENT_CHANGE: u32 = 2;
pub const SHK_EVENT_TEXT: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkMenuEvent {
    pub size: u32,
    pub ty: u32,
    pub item: ShkMenuItem,
    pub reserved: u32,
    pub value: f64,
    pub text: *const c_char,
}

pub type ShkMenuEventFn = unsafe extern "C" fn(*mut c_void, *const ShkMenuEvent);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShkMenuItemDesc {
    pub size: u32,
    pub kind: u32,
    pub flags: u32,
    pub option_count: u32,
    pub label: *const c_char,
    pub hint: *const c_char,
    pub reason: *const c_char,
    pub right: *const c_char,
    pub options: *const *const c_char,
    pub text: *const c_char,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub target: ShkMenuPage,
    pub hotkey: u32,
    pub on_event: Option<ShkMenuEventFn>,
    pub user: *mut c_void,
}

#[repr(C)]
pub struct ShkMenuService {
    pub size: u32,
    pub version: u32,
    pub add_page: unsafe extern "C" fn(*mut ShkPlugin, ShkMenuPage, *const c_char, *const c_char) -> ShkMenuPage,
    pub add_item: unsafe extern "C" fn(*mut ShkPlugin, ShkMenuPage, *const ShkMenuItemDesc) -> ShkMenuItem,
    pub set_value: unsafe extern "C" fn(ShkMenuItem, f64) -> i32,
    pub set_text: unsafe extern "C" fn(ShkMenuItem, *const c_char, *const c_char, *const c_char) -> i32,
    pub set_enabled: unsafe extern "C" fn(ShkMenuItem, i32, *const c_char) -> i32,
    pub clear_page: unsafe extern "C" fn(ShkMenuPage) -> i32,
    pub remove_owner: unsafe extern "C" fn(*mut ShkPlugin) -> i32,
    pub set_hud: unsafe extern "C" fn(*mut ShkPlugin, u32, *const c_char) -> i32,
    pub is_open: unsafe extern "C" fn() -> i32,
    pub toast: unsafe extern "C" fn(*mut ShkPlugin, *const c_char, f32) -> i32,
}

// SAFETY: the service table is immutable function pointers.
unsafe impl Sync for ShkMenuService {}
unsafe impl Sync for ShkHost {}
unsafe impl Send for ShkHost {}

/// Text of a fixed-size NUL-terminated byte array.
pub fn fixed_str(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}
