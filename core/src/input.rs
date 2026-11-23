//! Input read by the hook itself: the keyboard (held keys of the process's foreground window,
//! turned into key events with auto-repeat and typed characters) and gamepads (XInput). While a
//! plugin captures input (an open menu) the game's XInput reads report neutral pads.

use crate::hook;
use scripthook_sdk::ffi::{ShkKeyEvent, ShkPad};
use shk_sys::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

static HELD: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
pub static CAPTURED: AtomicBool = AtomicBool::new(false);
static XINPUT_GET: AtomicUsize = AtomicUsize::new(0);
static XINPUT_HOOKED: AtomicBool = AtomicBool::new(false);
static SYNTH: Mutex<Option<ShkPad>> = Mutex::new(None);
static SYNTH_KEYS: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());

pub fn key_held(vk: u32) -> bool {
    HELD[(vk as usize >> 6) & 3].load(Ordering::Relaxed) & (1 << (vk & 63)) != 0
}

fn set_held(vk: u32, on: bool) {
    let (w, b) = ((vk as usize >> 6) & 3, vk & 63);
    if on {
        HELD[w].fetch_or(1 << b, Ordering::Relaxed);
    } else {
        HELD[w].fetch_and(!(1 << b), Ordering::Relaxed);
    }
}

/// `SHK_INPUT=synthetic:<buttons hex>,<lx>,<ly>`: pad 0 reports that state (tests).
pub fn init_from_env() {
    if let Ok(v) = std::env::var("SHK_INPUT") {
        if let Some(spec) = v.strip_prefix("synthetic:") {
            let f: Vec<&str> = spec.split(',').collect();
            let mut p = ShkPad { buttons: u32::from_str_radix(f.first().copied().unwrap_or("0").trim_start_matches("0x"), 16).unwrap_or(0), ..Default::default() };
            p.left_x = f.get(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
            p.left_y = f.get(2).and_then(|x| x.parse().ok()).unwrap_or(0.0);
            *SYNTH.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
        }
    }
}

/// Injects a key press (automation, tests): down this frame, up the next.
pub fn inject_key(vk: u32) {
    let mut k = SYNTH_KEYS.lock().unwrap_or_else(|e| e.into_inner());
    k.push((vk, true));
    k.push((vk, false));
}

fn foreground_is_ours() -> bool {
    let mut pid = 0u32;
    // SAFETY: plain queries.
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return false;
        }
        GetWindowThreadProcessId(h, &mut pid);
        pid == GetCurrentProcessId()
    }
}

fn char_of(vk: u32, shift: bool) -> Option<char> {
    Some(match vk {
        0x30..=0x39 => {
            if shift {
                b")!@#$%^&*("[(vk - 0x30) as usize] as char
            } else {
                (b'0' + (vk - 0x30) as u8) as char
            }
        }
        0x41..=0x5A => {
            let c = vk as u8 as char;
            if shift {
                c
            } else {
                c.to_ascii_lowercase()
            }
        }
        0x60..=0x69 => (b'0' + (vk - 0x60) as u8) as char,
        0x20 => ' ',
        0xBD | 0x6D => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        0xBE | 0x6E => '.',
        0xBC => ',',
        _ => return None,
    })
}

pub struct Poll {
    repeat_at: [f64; 256],
}

impl Default for Poll {
    fn default() -> Self {
        Poll { repeat_at: [0.0; 256] }
    }
}

impl Poll {
    /// Reads the keyboard; returns this frame's key events (vk events, then character events).
    pub fn keyboard(&mut self, now: f64) -> Vec<ShkKeyEvent> {
        let mut out = Vec::new();
        let synth: Vec<(u32, bool)> = {
            let mut k = SYNTH_KEYS.lock().unwrap_or_else(|e| e.into_inner());
            // one press per frame: its down now, its up next frame
            let n = k.len().min(1 + k.first().map(|x| !x.1 as usize).unwrap_or(0));
            k.drain(..n).collect()
        };
        let injected: Vec<u32> = synth.iter().map(|x| x.0).collect();
        for (vk, down) in synth {
            set_held(vk, down);
            out.push(ShkKeyEvent { vk, down: down as u32, repeat: 0, ch: 0 });
            if down {
                if let Some(c) = char_of(vk, key_held(0x10)) {
                    out.push(ShkKeyEvent { vk: 0, down: 1, repeat: 0, ch: c as u32 });
                }
            }
        }
        let ours = foreground_is_ours();
        let shift = ours && unsafe { GetAsyncKeyState(0x10) } as u16 & 0x8000 != 0;
        for vk in 1u32..255 {
            if matches!(vk, 0x01..=0x06) || injected.contains(&vk) || (!ours && !key_held(vk)) {
                continue; // mouse buttons, injected keys, nothing to release
            }
            // SAFETY: plain query.
            let down = ours && unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
            let was = key_held(vk);
            if down != was {
                set_held(vk, down);
                out.push(ShkKeyEvent { vk, down: down as u32, repeat: 0, ch: 0 });
                if down {
                    self.repeat_at[vk as usize] = now + 0.4;
                    if let Some(c) = char_of(vk, shift) {
                        out.push(ShkKeyEvent { vk: 0, down: 1, repeat: 0, ch: c as u32 });
                    }
                }
            } else if down && now >= self.repeat_at[vk as usize] {
                self.repeat_at[vk as usize] = now + 0.05;
                out.push(ShkKeyEvent { vk, down: 1, repeat: 1, ch: 0 });
            }
        }
        out
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XState {
    packet: u32,
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

type XGet = unsafe extern "system" fn(u32, *mut XState) -> u32;

fn xinput() -> Option<XGet> {
    let mut f = XINPUT_GET.load(Ordering::Acquire);
    if f == 0 {
        for name in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
            // SAFETY: loading a system DLL and looking up its documented export.
            unsafe {
                let m = LoadLibraryW(wide(name).as_ptr());
                if !m.is_null() {
                    let p = GetProcAddress(m, c"XInputGetState".as_ptr() as *const u8);
                    if !p.is_null() {
                        f = p as usize;
                        break;
                    }
                }
            }
        }
        if f == 0 {
            return None;
        }
        XINPUT_GET.store(f, Ordering::Release);
    }
    // SAFETY: XInputGetState's documented signature.
    Some(unsafe { std::mem::transmute::<usize, XGet>(f) })
}

/// Pad `user` (0..3), not held back by a capture.
pub fn pad(user: u32) -> Option<ShkPad> {
    if user == 0 {
        if let Some(p) = *SYNTH.lock().unwrap_or_else(|e| e.into_inner()) {
            return Some(p);
        }
    }
    let get = xinput()?;
    let mut s = XState::default();
    // SAFETY: out structure of the documented layout.
    if unsafe { get(user, &mut s) } != 0 {
        return None;
    }
    let b = s.buttons as u32;
    let map = [(0x1, 0x40), (0x2, 0x80), (0x4, 0x100), (0x8, 0x200), (0x10, 0x1), (0x20, 0x2), (0x40, 0x1000), (0x80, 0x2000), (0x100, 0x400), (0x200, 0x800), (0x1000, 0x4), (0x2000, 0x8), (0x4000, 0x10), (0x8000, 0x20)];
    let buttons = map.iter().filter(|(x, _)| b & x != 0).fold(0, |a, (_, g)| a | g);
    let n = |v: i16| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    Some(ShkPad { buttons, left_trigger: s.lt as f32 / 255.0, right_trigger: s.rt as f32 / 255.0, left_x: n(s.lx), left_y: n(s.ly), right_x: n(s.rx), right_y: n(s.ry) })
}

unsafe extern "system" fn xinput_detour(user: u32, out: *mut XState) -> u32 {
    let orig: XGet = std::mem::transmute(XINPUT_GET.load(Ordering::Acquire));
    let r = orig(user, out);
    if r == 0 && CAPTURED.load(Ordering::Relaxed) && !out.is_null() {
        let p = (*out).packet;
        *out = XState { packet: p, ..Default::default() };
    }
    r
}

/// Starts or stops holding back the game's pad input (XInput) while a menu is open.
pub fn capture(on: bool) {
    CAPTURED.store(on, Ordering::Relaxed);
    if on && !XINPUT_HOOKED.swap(true, Ordering::AcqRel) {
        if let Some(f) = xinput() {
            if let Ok((_, t)) = hook::install(f as usize, xinput_detour as *const () as usize, 0) {
                XINPUT_GET.store(t, Ordering::Release);
            }
        }
    }
}

/// Keeps a C callback pointer usable from statics.
#[derive(Clone, Copy)]
pub struct Cb(pub usize, pub usize);
// SAFETY: plain addresses.
unsafe impl Send for Cb {}
pub fn ptr(p: *mut c_void) -> usize {
    p as usize
}
