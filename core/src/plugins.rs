//! Plugins: DLLs exporting `shk_plugin_load`, in `<scripthook folder>\plugins\` — either loose
//! (`plugins\x.dll`, plugin folder = `plugins\`) or in a folder of their own (`plugins\<name>\`
//! with the DLL named by `plugin.toml` / `mod.toml` `plugin = "x.dll"`, or the only DLL there).

use crate::host::{self, PluginCtx};
use crate::log;
use scripthook_sdk::ffi::*;
use shk_sys::*;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct Loaded {
    pub name: String,
    pub path: PathBuf,
    ctx: Box<PluginCtx>,
    module: usize,
    unload: Option<unsafe extern "C" fn()>,
}

// SAFETY: module handles and plain data.
unsafe impl Send for Loaded {}

static LOADED: Mutex<Vec<Loaded>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(100);

/// The plugin DLLs in `dir` (sorted by folder / file name).
pub fn find(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        if p.is_file() && name.to_ascii_lowercase().ends_with(".dll") {
            out.push((name.trim_end_matches(".dll").trim_end_matches(".DLL").to_string(), p));
        } else if p.is_dir() {
            let named = ["plugin.toml", "mod.toml"].iter().find_map(|m| {
                let t = std::fs::read_to_string(p.join(m)).ok()?;
                scripthook_sdk::config::toml::parse(&t).ok()?.str_of("plugin")
            });
            let dll = match named {
                Some(n) => Some(p.join(n)),
                None => {
                    let dlls: Vec<PathBuf> = std::fs::read_dir(&p).map(|r| r.flatten().map(|e| e.path()).filter(|x| x.extension().map(|e| e.eq_ignore_ascii_case("dll")).unwrap_or(false)).collect()).unwrap_or_default();
                    (dlls.len() == 1).then(|| dlls[0].clone())
                }
            };
            if let Some(d) = dll.filter(|d| d.is_file()) {
                out.push((name, d));
            }
        }
    }
    out
}

/// Loads one plugin and calls its `shk_plugin_load`.
pub fn load(name: &str, path: &Path) -> Result<(), String> {
    let w = wide(&path.display().to_string());
    // SAFETY: loading a DLL the user put in the plugins folder (that is what a plugin is).
    let m = unsafe { LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), 0x8) } as usize;
    if m == 0 {
        return Err(format!("LoadLibrary failed (error {})", unsafe { GetLastError() }));
    }
    // SAFETY: the documented exports.
    unsafe {
        let load = GetProcAddress(m as _, c"shk_plugin_load".as_ptr() as *const u8);
        if load.is_null() {
            FreeLibrary(m as _);
            return Err("no shk_plugin_load export (not a scripthook plugin)".into());
        }
        let unload = GetProcAddress(m as _, c"shk_plugin_unload".as_ptr() as *const u8);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        let mut ctx = Box::new(PluginCtx { id, name: name.to_string(), folder: CString::new(folder).unwrap_or_default() });
        let f: ShkPluginLoadFn = std::mem::transmute(load);
        let r = f(host::host(), &mut *ctx as *mut PluginCtx as *mut ShkPlugin);
        if r != 0 {
            host::remove_owner(id);
            FreeLibrary(m as _);
            return Err(format!("shk_plugin_load returned {r}"));
        }
        let unload = (!unload.is_null()).then(|| std::mem::transmute::<*mut std::ffi::c_void, unsafe extern "C" fn()>(unload));
        LOADED.lock().unwrap_or_else(|e| e.into_inner()).push(Loaded { name: name.to_string(), path: path.to_path_buf(), ctx, module: m, unload });
    }
    Ok(())
}

type ShkPluginLoadFn = unsafe extern "C" fn(*const ShkHost, *mut ShkPlugin) -> i32;

pub fn load_all(dir: &Path) -> usize {
    let mut n = 0;
    for (name, p) in find(dir) {
        match load(&name, &p) {
            Ok(()) => {
                log::info(&format!("plugin {name} loaded from {}", p.display()));
                n += 1;
            }
            Err(e) => log::warn(&format!("plugin {name} ({}): {e}", p.display())),
        }
    }
    n
}

/// Unloads every plugin (reverse order): their unload export, then their registrations.
pub fn unload_all() {
    let all: Vec<Loaded> = std::mem::take(&mut *LOADED.lock().unwrap_or_else(|e| e.into_inner()));
    for p in all.into_iter().rev() {
        if let Some(f) = p.unload {
            // SAFETY: the plugin's documented export.
            unsafe { f() };
        }
        host::remove_owner(p.ctx.id);
        log::info(&format!("plugin {} unloaded", p.name));
        // the DLL stays mapped: a callback may still be running on another thread at exit
        let _ = p.module;
    }
}

pub fn names() -> Vec<String> {
    LOADED.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|p| p.name.clone()).collect()
}
