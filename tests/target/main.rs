//! `shk-test-target`: gta-vi-scripthook's synthetic stand-in for a game (no game code or data).
//! A natives table (found by pattern), a script thread calling a tick function, a player record,
//! and a D3D12 window (hidden) presenting every frame through a DXGI swap chain.
//!
//! `shk-test-target --frames N --summary <file>`: runs N frames, writes a summary line.

use shk_sys::*;
use shk_tests::target::{self, NATIVES};
use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

#[repr(C)]
struct Player {
    magic: [u8; 16],
    money: i32,
    health: f32,
    armour: f32,
    pos: [f32; 3],
    vel: [f32; 3],
    yaw: f32,
    invincible: i32,
    wanted: i32,
}

#[repr(C)]
struct Registry {
    magic: [u8; 16],
    buckets: [usize; 8],
}

#[repr(C)]
struct TickRef {
    magic: [u8; 16],
    tick: usize,
}

#[repr(C)]
struct Bucket {
    next: usize,
    h: [usize; 7],
    count: u32,
    pad: u32,
    hashes: [u64; 7],
}

/// The call context (default layout).
#[repr(C)]
struct Ctx {
    ret: *mut u64,
    argc: u32,
    _p: u32,
    args: *mut u64,
    data_count: u32,
    _p2: u32,
    fix_out: [*mut f32; 4],
    fix_in: [[f32; 4]; 4],
}

static mut PLAYER: Player = Player { magic: *b"SHK-TGT-PLAYER!!", money: 12345, health: 100.0, armour: 50.0, pos: [0.0, 0.0, 10.0], vel: [0.0; 3], yaw: 0.0, invincible: 0, wanted: 0 };
static mut REGISTRY: Registry = Registry { magic: *b"SHK-TGT-NATIVES!", buckets: [0; 8] };
static mut TICKREF: TickRef = TickRef { magic: *b"SHK-TGT-TICKREF!", tick: 0 };

static TICKS: AtomicU64 = AtomicU64::new(0);
static SCRIPT_TID: AtomicU32 = AtomicU32::new(0);
static HOUR: AtomicI32 = AtomicI32::new(-1);
static VEHICLES: AtomicI32 = AtomicI32::new(0);
static VMODEL: AtomicU32 = AtomicU32::new(0);
static IN_VEHICLE: AtomicI32 = AtomicI32::new(0);
static STAT_SETS: AtomicI32 = AtomicI32::new(0);
static REQUESTED: AtomicU32 = AtomicU32::new(0);
static POLLS: AtomicI32 = AtomicI32::new(0);
static WANTED_PENDING: AtomicI32 = AtomicI32::new(0);
static MAX_WANTED: AtomicI32 = AtomicI32::new(5);
static WEATHER: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static VPOS: std::sync::Mutex<[f32; 3]> = std::sync::Mutex::new([0.0; 3]);
const PED: i32 = 100;
const PLAYER_ID: i32 = 7;

#[allow(static_mut_refs)]
fn player() -> &'static mut Player {
    // SAFETY: the test target's own record (races are harmless here).
    unsafe { &mut PLAYER }
}

unsafe fn ai(c: *mut Ctx, k: usize) -> i32 {
    *(*c).args.add(k) as u32 as i32
}
unsafe fn af(c: *mut Ctx, k: usize) -> f32 {
    f32::from_bits(*(*c).args.add(k) as u32)
}
unsafe fn ri(c: *mut Ctx, v: i32) {
    *(*c).ret = v as u32 as u64;
}
unsafe fn rv(c: *mut Ctx, v: [f32; 3]) {
    for (k, x) in v.iter().enumerate() {
        *(*c).ret.add(k) = x.to_bits() as u64;
    }
}

fn native(name: &str, c: *mut Ctx) {
    // SAFETY: the context the hook built.
    unsafe {
        let p = player();
        match name {
            "PLAYER::PLAYER_ID" => ri(c, PLAYER_ID),
            "PLAYER::PLAYER_PED_ID" => ri(c, PED),
            "PED::GET_VEHICLE_PED_IS_IN" => ri(c, if ai(c, 0) == PED { IN_VEHICLE.load(Ordering::SeqCst) } else { 0 }),
            "PLAYER::SET_PLAYER_INVINCIBLE" => p.invincible = (*(*c).args.add(1) & 0xFF != 0) as i32,
            "ENTITY::GET_ENTITY_COORDS" => rv(c, if ai(c, 0) == PED { p.pos } else { *VPOS.lock().unwrap() }),
            "ENTITY::SET_ENTITY_COORDS_NO_OFFSET" => {
                let v = [af(c, 1), af(c, 2), af(c, 3)];
                if ai(c, 0) == PED {
                    p.pos = v
                } else {
                    *VPOS.lock().unwrap() = v
                }
            }
            "ENTITY::GET_ENTITY_HEADING" => *(*c).ret = p.yaw.to_bits() as u64,
            "ENTITY::SET_ENTITY_HEADING" => p.yaw = af(c, 1),
            "ENTITY::SET_ENTITY_VELOCITY" => p.vel = [af(c, 1), af(c, 2), af(c, 3)],
            "CAM::GET_GAMEPLAY_CAM_ROT" => rv(c, [0.0; 3]),
            "ENTITY::GET_OFFSET_FROM_ENTITY_IN_WORLD_COORDS" => rv(c, [p.pos[0] + af(c, 1), p.pos[1] + af(c, 2), p.pos[2] + af(c, 3)]),
            "STREAMING::REQUEST_MODEL" => {
                REQUESTED.store(*(*c).args as u32, Ordering::SeqCst);
                POLLS.store(0, Ordering::SeqCst);
            }
            "STREAMING::HAS_MODEL_LOADED" => ri(c, (*(*c).args as u32 == REQUESTED.load(Ordering::SeqCst) && POLLS.fetch_add(1, Ordering::SeqCst) >= 2) as i32),
            "STREAMING::SET_MODEL_AS_NO_LONGER_NEEDED" => {}
            "VEHICLE::CREATE_VEHICLE" => {
                let n = VEHICLES.fetch_add(1, Ordering::SeqCst) + 1;
                VMODEL.store(*(*c).args as u32, Ordering::SeqCst);
                *VPOS.lock().unwrap() = [af(c, 1), af(c, 2), af(c, 3)];
                ri(c, 200 + n);
            }
            "VEHICLE::SET_VEHICLE_ON_GROUND_PROPERLY" => ri(c, 1),
            "PED::SET_PED_INTO_VEHICLE" => {
                if ai(c, 0) == PED {
                    IN_VEHICLE.store(ai(c, 1), Ordering::SeqCst)
                }
            }
            "MISC::SET_WEATHER_TYPE_NOW_PERSIST" => {
                let s = *(*c).args as *const std::ffi::c_char;
                *WEATHER.lock().unwrap() = if s.is_null() { "null".into() } else { std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned() };
            }
            "CLOCK::SET_CLOCK_TIME" => HOUR.store(ai(c, 0), Ordering::SeqCst),
            "PLAYER::SET_PLAYER_WANTED_LEVEL" => WANTED_PENDING.store(ai(c, 1), Ordering::SeqCst),
            "PLAYER::SET_PLAYER_WANTED_LEVEL_NOW" => p.wanted = WANTED_PENDING.load(Ordering::SeqCst),
            "PLAYER::SET_MAX_WANTED_LEVEL" => MAX_WANTED.store(ai(c, 0), Ordering::SeqCst),
            "PLAYER::CLEAR_PLAYER_WANTED_LEVEL" => p.wanted = 0,
            "STATS::STAT_GET_INT" => {
                let ok = *(*c).args as u32 == scripthook_sdk::joaat("SP0_TOTAL_CASH");
                if ok {
                    *(*(*c).args.add(1) as *mut i32) = p.money;
                }
                ri(c, ok as i32);
            }
            "STATS::STAT_SET_INT" => {
                let ok = *(*c).args as u32 == scripthook_sdk::joaat("SP0_TOTAL_CASH");
                if ok {
                    p.money = ai(c, 1);
                    STAT_SETS.fetch_add(1, Ordering::SeqCst);
                }
                ri(c, ok as i32);
            }
            "TEST::VEC_SCALE" => {
                let s = af(c, 3);
                rv(c, [af(c, 0) * s, af(c, 1) * s, af(c, 2) * s]);
            }
            "TEST::GET_POS_FIXUP" => {
                (*c).fix_out[0] = *(*c).args as *mut f32;
                (*c).fix_in[0] = [p.pos[0], p.pos[1], p.pos[2], 0.0];
                (*c).data_count = 1;
                ri(c, 1);
            }
            "TEST::THREAD_ID" => ri(c, GetCurrentThreadId() as i32),
            _ => {}
        }
    }
}

macro_rules! handlers {
    ($($i:literal),*) => {
        [$({
            extern "C" fn h(c: *mut Ctx) {
                native(NATIVES[$i].0, c)
            }
            h as *const () as usize
        }),*]
    };
}

/// The game's per-tick script function (the hook runs scripts before it).
#[inline(never)]
#[no_mangle]
pub extern "C" fn target_script_tick(n: i64) {
    TICKS.fetch_add(1, Ordering::SeqCst);
    if std::hint::black_box(n) < -1000 {
        println!("negative tick");
    }
}

fn register_natives() {
    let hs: [usize; 28] = handlers!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27);
    assert_eq!(hs.len(), NATIVES.len());
    // SAFETY: the target's own registry, filled before anything reads it.
    #[allow(static_mut_refs)]
    unsafe {
        for (i, (name, _, _)) in NATIVES.iter().enumerate() {
            let h = target::id(name);
            let slot = &mut REGISTRY.buckets[(h % target::BUCKETS) as usize];
            let mut node = *slot as *mut Bucket;
            if node.is_null() || (*node).count == 7 {
                let b = Box::into_raw(Box::new(Bucket { next: *slot, h: [0; 7], count: 0, pad: 0, hashes: [0; 7] }));
                *slot = b as usize;
                node = b;
            }
            let k = (*node).count as usize;
            (*node).h[k] = hs[i];
            (*node).hashes[k] = h ^ target::HASH_XOR;
            (*node).count += 1;
        }
        TICKREF.tick = target_script_tick as *const () as usize;
        std::hint::black_box(&REGISTRY);
        std::hint::black_box(&TICKREF);
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: usize, l: isize) -> isize {
    DefWindowProcW(h, m, w, l)
}

unsafe extern "system" fn script_thread(_: *mut c_void) -> u32 {
    SCRIPT_TID.store(GetCurrentThreadId(), Ordering::SeqCst);
    let mut n = 0i64;
    loop {
        target_script_tick(n);
        n += 1;
        Sleep(4);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let frames: u32 = get("--frames").and_then(|x| x.parse().ok()).unwrap_or(300);
    let summary = get("--summary");
    register_natives();
    // SAFETY: Win32 / D3D12 calls with the documented arguments; this is a test program.
    unsafe {
        CloseHandle(CreateThread(std::ptr::null_mut(), 0, script_thread, std::ptr::null_mut(), 0, std::ptr::null_mut()));
        let cls = wide("shk-test-target");
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, style: 0, lpfnWndProc: wndproc, cbClsExtra: 0, cbWndExtra: 0, hInstance: inst, hIcon: std::ptr::null_mut(), hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: cls.as_ptr(), hIconSm: std::ptr::null_mut() };
        RegisterClassExW(&wc);
        let (w, h) = (1280u32, 720u32);
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, cls.as_ptr(), cls.as_ptr(), WS_POPUP, 0, 0, w as i32, h as i32, std::ptr::null_mut(), std::ptr::null_mut(), inst, std::ptr::null_mut());
        let mut dev: Unk = std::ptr::null_mut();
        assert!(D3D12CreateDevice(std::ptr::null_mut(), 0xb000, &IID_ID3D12Device, &mut dev) >= 0, "no D3D12 device");
        let qd = D3D12_COMMAND_QUEUE_DESC::default();
        let mut q: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, *const D3D12_COMMAND_QUEUE_DESC, *const GUID, *mut Unk) -> HRESULT>(dev, 8)(dev, &qd, &IID_ID3D12CommandQueue, &mut q);
        let mut f: Unk = std::ptr::null_mut();
        CreateDXGIFactory2(0, &IID_IDXGIFactory4, &mut f);
        let sd = DXGI_SWAP_CHAIN_DESC1 { Width: w, Height: h, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleCount: 1, BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT, BufferCount: 2, SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD, ..Default::default() };
        let mut sc1: Unk = std::ptr::null_mut();
        let hr = vfn::<unsafe extern "system" fn(Unk, Unk, HWND, *const DXGI_SWAP_CHAIN_DESC1, *const c_void, Unk, *mut Unk) -> HRESULT>(f, 15)(f, q, hwnd, &sd, std::ptr::null(), std::ptr::null_mut(), &mut sc1);
        assert!(hr >= 0 && !sc1.is_null(), "CreateSwapChainForHwnd {hr:#x}");
        let sc = query(sc1, &IID_IDXGISwapChain3).unwrap();
        let hd = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 2, Flags: 0, NodeMask: 0 };
        let mut heap: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, *const D3D12_DESCRIPTOR_HEAP_DESC, *const GUID, *mut Unk) -> HRESULT>(dev, 14)(dev, &hd, &IID_ID3D12DescriptorHeap, &mut heap);
        let mut rtv0 = 0usize;
        vfn::<unsafe extern "system" fn(Unk, *mut usize) -> *mut usize>(heap, 9)(heap, &mut rtv0);
        let step = vfn::<unsafe extern "system" fn(Unk, u32) -> u32>(dev, 15)(dev, D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        let mut alloc: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u32, *const GUID, *mut Unk) -> HRESULT>(dev, 9)(dev, 0, &IID_ID3D12CommandAllocator, &mut alloc);
        let mut list: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u32, u32, Unk, Unk, *const GUID, *mut Unk) -> HRESULT>(dev, 12)(dev, 0, 0, alloc, std::ptr::null_mut(), &IID_ID3D12GraphicsCommandList, &mut list);
        vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(list, 9)(list);
        let mut fence: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u64, u32, *const GUID, *mut Unk) -> HRESULT>(dev, 36)(dev, 0, 0, &IID_ID3D12Fence, &mut fence);
        let mut fv = 0u64;
        let t0 = now();
        for fr in 0..frames {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // game logic
            let p = player();
            if fr == 60 {
                p.money = 12400;
            }
            if fr >= 100 && p.invincible == 0 {
                p.health -= 0.5;
            }
            p.vel[2] -= 0.02;
            for k in 0..3 {
                p.pos[k] += p.vel[k];
            }
            if p.pos[2] < 0.0 {
                p.pos[2] = 0.0;
                p.vel[2] = 0.0;
            }
            // render: a background and a bar, then present
            let idx = vfn::<unsafe extern "system" fn(Unk) -> u32>(sc, 36)(sc) as usize;
            let mut buf: Unk = std::ptr::null_mut();
            vfn::<unsafe extern "system" fn(Unk, u32, *const GUID, *mut Unk) -> HRESULT>(sc, 9)(sc, idx as u32, &IID_ID3D12Resource, &mut buf);
            let rtv = rtv0 + idx * step;
            vfn::<unsafe extern "system" fn(Unk, Unk, *const c_void, usize)>(dev, 20)(dev, buf, std::ptr::null(), rtv);
            vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(alloc, 8)(alloc);
            vfn::<unsafe extern "system" fn(Unk, Unk, Unk) -> HRESULT>(list, 10)(list, alloc, std::ptr::null_mut());
            let b = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
            vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(list, 26)(list, 1, &b);
            let bg = [0.30f32, 0.10, 0.20, 1.0];
            vfn::<unsafe extern "system" fn(Unk, usize, *const f32, u32, *const i32)>(list, 48)(list, rtv, bg.as_ptr(), 0, std::ptr::null());
            let fg = [0.9f32, 0.6, 0.1, 1.0];
            let bar = [40i32, 600, 40 + (fr % 1000) as i32, 640];
            vfn::<unsafe extern "system" fn(Unk, usize, *const f32, u32, *const i32)>(list, 48)(list, rtv, fg.as_ptr(), 1, bar.as_ptr());
            let b = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
            vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(list, 26)(list, 1, &b);
            vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(list, 9)(list);
            let lists = [list];
            vfn::<unsafe extern "system" fn(Unk, u32, *const Unk)>(q, 10)(q, 1, lists.as_ptr());
            release(buf);
            vfn::<unsafe extern "system" fn(Unk, u32, u32) -> HRESULT>(sc, 8)(sc, 0, 0);
            fv += 1;
            vfn::<unsafe extern "system" fn(Unk, Unk, u64) -> HRESULT>(q, 14)(q, fence, fv);
            while vfn::<unsafe extern "system" fn(Unk) -> u64>(fence, 8)(fence) < fv {
                Sleep(0);
            }
            // about 60 frames a second
            let target = t0 + (fr + 1) as f64 / 60.0;
            while now() < target {
                Sleep(1);
            }
        }
        // the hook (when loaded) unloads its plugins now, while the process is intact
        let m = GetModuleHandleW(wide("scripthook.dll").as_ptr());
        let hooked = !m.is_null();
        if hooked {
            let f = GetProcAddress(m, c"shk_core_shutdown".as_ptr() as *const u8);
            if !f.is_null() {
                std::mem::transmute::<*mut c_void, extern "C" fn()>(f)();
            }
        }
        let p = player();
        let vp = *VPOS.lock().unwrap();
        let line = format!(
            "target: frames={frames} hooked={} money={} health={:.2} armour={:.2} pos=({:.3},{:.3},{:.3}) invincible={} wanted={} max_wanted={} hour={} weather={} vehicles={} vmodel={} in_vehicle={} vpos=({:.2},{:.2},{:.2}) stat_sets={} script_ticks={} script_tid={}",
            hooked as i32,
            p.money,
            p.health,
            p.armour,
            p.pos[0],
            p.pos[1],
            p.pos[2],
            p.invincible,
            p.wanted,
            MAX_WANTED.load(Ordering::SeqCst),
            HOUR.load(Ordering::SeqCst),
            {
                let w = WEATHER.lock().unwrap().clone();
                if w.is_empty() {
                    "none".to_string()
                } else {
                    w
                }
            },
            VEHICLES.load(Ordering::SeqCst),
            VMODEL.load(Ordering::SeqCst),
            IN_VEHICLE.load(Ordering::SeqCst),
            vp[0],
            vp[1],
            vp[2],
            STAT_SETS.load(Ordering::SeqCst),
            TICKS.load(Ordering::SeqCst),
            SCRIPT_TID.load(Ordering::SeqCst)
        );
        println!("{line}");
        if let Some(s) = summary {
            let _ = std::fs::write(s, &line);
        }
        DestroyWindow(hwnd);
        std::process::exit(0);
    }
}
