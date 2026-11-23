//! gta-vi-scripthook's hook DLL (`scripthook.dll`). It runs inside the game process and does
//! everything itself:
//!
//! * finds the game module in memory ([`mem`], [`locate`]: the loader's list or images mapped by
//!   other means);
//! * locates the natives handler table and the script tick by data-driven patterns
//!   (`natives.tsv`) and calls natives with its own call context ([`natives`]);
//! * runs script fibers from its own hook of the script tick, or before each present as a
//!   fallback ([`scripts`], [`hook`]);
//! * hooks `IDXGISwapChain::Present` and draws with its own D3D12 renderer and font ([`overlay`]);
//! * reads the keyboard and XInput itself ([`input`]);
//! * does memory, pattern and inline-hook work with its own x86-64 decoder ([`x64`], [`hook`]);
//! * loads plugins from `plugins\` and serves them `scripthook.h` ([`host`], [`plugins`]).
//!
//! Load it with `shk-loader` (your own single-player game only) or any runtime's plugin
//! mechanism that loads DLLs (it also exports Sonoma Runtime's `srt_plugin_init`, a no-op
//! convenience: the hook starts from `DllMain`).

pub mod auto;
pub mod hook;
pub mod host;
pub mod input;
pub mod locate;
pub mod log;
pub mod mem;
pub mod natives;
pub mod overlay;
pub mod plugins;
pub mod scripts;
pub mod x64;

use scripthook_sdk::config::{toml, Value};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
pub(crate) static TEST_SCRIPTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `scripthook.toml` (every key optional).
#[derive(Debug, Clone)]
pub struct Config {
    /// the name natives.tsv locations use for the game's executable
    pub game_module: String,
    pub plugins: PathBuf,
    pub natives: Vec<PathBuf>,
    pub overlay: bool,
    /// seconds to keep looking for the script tick (the game may map its code later)
    pub tick_wait: f64,
}

impl Config {
    pub fn load(dir: &Path) -> Config {
        let v = std::fs::read_to_string(dir.join("scripthook.toml")).ok().and_then(|t| toml::parse(&t).ok()).unwrap_or(Value::Table(Vec::new()));
        let rel = |p: String| -> PathBuf {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                p
            } else {
                dir.join(p)
            }
        };
        let mut natives: Vec<PathBuf> = v.strings_of("natives").into_iter().map(rel).collect();
        if natives.is_empty() {
            natives = vec![dir.join("natives.tsv"), dir.join("data").join("natives.tsv")];
        }
        if let Ok(e) = std::env::var("SHK_NATIVES") {
            natives.extend(e.split(';').filter(|x| !x.is_empty()).map(PathBuf::from));
        }
        let plugins = std::env::var("SHK_PLUGINS").map(PathBuf::from).unwrap_or_else(|_| rel(v.str_of("plugins").unwrap_or_else(|| "plugins".into())));
        Config {
            game_module: v.str_of("game_module").unwrap_or_default(),
            plugins,
            natives,
            overlay: v.bool_of("overlay").unwrap_or(true) && std::env::var("SHK_OVERLAY").map(|x| x != "0").unwrap_or(true),
            tick_wait: v.f64_of("tick_wait").unwrap_or(60.0),
        }
    }
}

static STARTED: AtomicBool = AtomicBool::new(false);

/// Starts the hook with its files in `dir` (called on a thread of its own).
pub fn start(dir: &Path) {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let logp = std::env::var("SHK_LOG").map(PathBuf::from).unwrap_or_else(|_| dir.join("scripthook.log"));
    log::open(&logp);
    let mut exe = [0u16; 260];
    // SAFETY: out buffer.
    unsafe { shk_sys::GetModuleFileNameW(std::ptr::null_mut(), exe.as_mut_ptr(), 260) };
    let exe = shk_sys::from_wide(&exe);
    log::info(&format!("gta-vi-scripthook {} in {exe} (single-player / offline only)", env!("CARGO_PKG_VERSION")));
    let cfg = Config::load(dir);
    host::set_identity(&cfg.game_module);
    locate::set_game_module(&cfg.game_module);
    input::init_from_env();
    // natives
    let texts: Vec<(&str, String)> = cfg.natives.iter().filter_map(|p| std::fs::read_to_string(p).ok().map(|t| ("natives.tsv", t))).collect();
    let tick = if texts.is_empty() {
        log::warn("no natives.tsv: natives are unavailable (docs/natives-database.md)");
        None
    } else {
        let (n, errs) = natives::Natives::from_tsv(&texts);
        for e in errs {
            log::warn(&format!("natives: {e}"));
        }
        log::info(&format!("natives: {} in the database, {} with an identifier; table {}; script tick {}", n.db.natives.len(), n.known(), if n.table.is_some() { "given" } else { "not given" }, if n.tick.is_some() { "given" } else { "not given" }));
        let t = n.tick.clone();
        *host::NATIVES.lock().unwrap_or_else(|e| e.into_inner()) = Some(n);
        t
    };
    scripts::set_mode(scripts::Mode::PresentFallback, if tick.is_some() { "the script tick was not found yet" } else { "the script tick's location is not given (@script_tick)" });
    // overlay
    if cfg.overlay {
        match overlay::install() {
            Ok(m) => log::info(&format!("overlay: {m}")),
            Err(e) => log::warn(&format!("overlay: not available: {e}")),
        }
        overlay::set_frame_hook(frame_hook);
    }
    auto::init();
    // plugins
    let n = plugins::load_all(&cfg.plugins);
    log::info(&format!("{n} plugin(s) loaded from {}", cfg.plugins.display()));
    // the script tick (the game may map its code a little later)
    if let Some(t) = tick {
        let t0 = shk_sys::now();
        loop {
            match install_tick(&t) {
                Ok(at) => {
                    scripts::set_mode(scripts::Mode::GameThread, "");
                    log::info(&format!("scripts: running on the game's script thread (tick hooked at {at:#x})"));
                    break;
                }
                Err(e) if shk_sys::now() - t0 > cfg.tick_wait => {
                    log::warn(&format!("scripts: present-time fallback: {e}"));
                    scripts::set_mode(scripts::Mode::PresentFallback, &e);
                    break;
                }
                // SAFETY: plain sleep.
                Err(_) => unsafe { shk_sys::Sleep(250) },
            }
        }
    } else {
        log::info("scripts: present-time fallback (no @script_tick)");
    }
}

extern "C" fn on_tick() {
    scripts::tick(scripts::Mode::GameThread);
}

fn install_tick(loc: &Value) -> Result<usize, String> {
    let at = locate::resolve(loc).map_err(|e| format!("script tick: {e}"))?;
    let (stub, slot) = hook::call_before_stub(at, on_tick as *const () as usize).ok_or("no memory near the script tick")?;
    hook::install_ex(at, stub, 0, &|t| hook::set_stub_target(slot, t)).map_err(|e| format!("script tick at {at:#x}: {e}"))?;
    Ok(at)
}

fn frame_hook(dl: &mut overlay::DrawList, w: u32, h: u32) {
    auto::step();
    host::frame(dl, w, h);
}

/// Unloads the plugins (process exit or the host's shutdown).
pub fn shutdown() {
    if STARTED.load(Ordering::Acquire) {
        plugins::unload_all();
        let (c, f) = host::native_stats();
        log::info(&format!("shutdown: {} presents, {} drawn, {} script ticks, {c} native calls ({f} failed), {} hooks", overlay::PRESENTS.load(Ordering::Relaxed), overlay::DRAWN.load(Ordering::Relaxed), scripts::ticks(), hook::count()));
    }
}

fn own_dir() -> PathBuf {
    let mut m: shk_sys::HMODULE = std::ptr::null_mut();
    let mut buf = [0u16; 520];
    // SAFETY: the module containing this function; out buffers.
    unsafe {
        shk_sys::GetModuleHandleExW(0x4 | 0x2, own_dir as *const u16, &mut m);
        shk_sys::GetModuleFileNameW(m, buf.as_mut_ptr(), 520);
    }
    PathBuf::from(shk_sys::from_wide(&buf)).parent().map(Path::to_path_buf).unwrap_or_default()
}

unsafe extern "system" fn init_thread(_: *mut c_void) -> u32 {
    let dir = own_dir();
    let _ = std::panic::catch_unwind(|| start(&dir));
    0
}

/// Windows calls this when the DLL is loaded: the hook starts on a thread of its own (nothing
/// heavy runs under the loader lock).
#[no_mangle]
pub unsafe extern "system" fn DllMain(module: *mut c_void, reason: u32, _: *mut c_void) -> i32 {
    if reason == 1 {
        shk_sys::DisableThreadLibraryCalls(module);
        let h = shk_sys::CreateThread(std::ptr::null_mut(), 0, init_thread, std::ptr::null_mut(), 0, std::ptr::null_mut());
        if !h.is_null() {
            shk_sys::CloseHandle(h);
        }
    }
    1
}

/// Unloads the plugins; for hosts and test programs that end the process cleanly.
#[no_mangle]
pub extern "C" fn shk_core_shutdown() {
    shutdown();
}

/// Sonoma Runtime's plugin entry point (an optional way to load the hook): nothing to do, the
/// hook started from DllMain and uses none of the runtime's API.
#[no_mangle]
pub extern "C" fn srt_plugin_init(_api: *const c_void, _me: *mut c_void) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn srt_plugin_shutdown() {
    shutdown();
}
