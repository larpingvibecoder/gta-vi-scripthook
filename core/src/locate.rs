//! Locations of `natives.tsv` directives and memory cheats: `module` + `export` / `pattern`
//! (`section`, `nth`, `rip = { disp, len }`) / `offset`, or `address`; then `offset`; then a
//! pointer `chain`. Module names match the Windows loader's list, the export-directory name of
//! images mapped by other means, and the configured game module (`""` / `"game"` / its name).

use crate::mem::{self, Module};
use scripthook_sdk::config::Value;
use std::sync::Mutex;

/// The game module's configured name and the modules seen (refreshed on misses).
struct Registry {
    game: String,
    mods: Vec<Module>,
}

static REG: Mutex<Registry> = Mutex::new(Registry { game: String::new(), mods: Vec::new() });

pub fn set_game_module(name: &str) {
    REG.lock().unwrap_or_else(|e| e.into_inner()).game = name.to_string();
}

pub fn game_module_name() -> String {
    REG.lock().unwrap_or_else(|e| e.into_inner()).game.clone()
}

fn find_in(mods: &[Module], game: &str, name: &str) -> Option<Module> {
    let n = name.to_ascii_lowercase();
    let stem = |s: &str| s.to_ascii_lowercase().trim_end_matches(".exe").trim_end_matches(".dll").to_string();
    if let Some(m) = mods.iter().find(|m| m.name.to_ascii_lowercase() == n || stem(&m.name) == stem(name)) {
        return Some(m.clone());
    }
    if n.is_empty() || n == "game" || (!game.is_empty() && stem(game) == stem(name)) {
        // the game: an unlisted executable image (mapped by a runtime) if there is one, else
        // the process's own executable
        let unlisted: Vec<&Module> = mods.iter().filter(|m| !m.listed && m.is_exe).collect();
        if let Some(m) = unlisted.iter().max_by_key(|m| m.size) {
            return Some((*m).clone());
        }
        return mods.iter().find(|m| m.listed && m.is_exe).cloned();
    }
    None
}

/// A module by name (see the module docs).
pub fn module(name: &str) -> Option<Module> {
    let mut r = REG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = find_in(&r.mods, &r.game, name) {
        return Some(m);
    }
    r.mods = mem::modules();
    find_in(&r.mods, &r.game, name)
}

pub fn modules() -> Vec<Module> {
    let mut r = REG.lock().unwrap_or_else(|e| e.into_inner());
    r.mods = mem::modules();
    r.mods.clone()
}

/// Resolves a location table to an address.
pub fn resolve(v: &Value) -> Result<usize, String> {
    if let Some(u) = v.str_of("unknown") {
        return Err(u);
    }
    let offset = v.i64_of("offset").unwrap_or(0);
    let mut a = if let Some(addr) = v.get("address").and_then(Value::as_i64) {
        addr as usize
    } else {
        let mname = v.str_of("module").unwrap_or_default();
        let m = module(&mname).ok_or_else(|| format!("module {mname} not found"))?;
        if let Some(e) = v.str_of("export") {
            m.export(&e).ok_or_else(|| format!("no export {e} in {}", m.name))?
        } else if let Some(p) = v.str_of("pattern") {
            let pat = mem::parse_pattern(&p)?;
            let sec = v.str_of("section");
            let nth = v.i64_of("nth").unwrap_or(0).max(0) as usize;
            let hits = mem::scan_module(&m, sec.as_deref(), &pat, nth + 1);
            let mut x = *hits.get(nth).ok_or_else(|| format!("pattern not found in {}{}", m.name, sec.map(|s| format!(" {s}")).unwrap_or_default()))?;
            if let Some(r) = v.get("rip") {
                x = mem::rip_target(x, r.i64_of("disp").unwrap_or(3) as usize, r.i64_of("len").unwrap_or(7) as usize).ok_or("RIP displacement unreadable")?;
            }
            x
        } else {
            m.base
        }
    };
    a = (a as i64 + offset) as usize;
    if let Some(ch) = v.get("chain").and_then(Value::as_array) {
        let offs: Vec<i64> = ch.iter().filter_map(Value::as_i64).collect();
        a = mem::resolve_chain(a, &offs).ok_or("pointer chain broken")?;
    }
    Ok(a)
}
