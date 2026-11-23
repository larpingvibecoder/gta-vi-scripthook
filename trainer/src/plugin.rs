//! The trainer plugin: loads the definitions, the trainer data and the state, builds its menu
//! pages at the first frame (when every plugin is loaded), runs toggles on the game's script
//! thread and memory cheats every frame.

use crate::auto::Auto;
use crate::cheats::CheatDefs;
use crate::features;
use crate::runtime::Trainer;
use crate::ui;
use scripthook_sdk::config::toml;
use scripthook_sdk::menu::Menu;
use scripthook_sdk::{self as shk, Host};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

pub struct TrainerPlugin {
    slot: Arc<OnceLock<Arc<Trainer>>>,
}

/// Trainer data folders: `SHK_TRAINER_DATA` / `SRT_TRAINER_DATA` (`;`-separated), `data` in
/// trainer.toml (relative to the plugin folder), and `<plugin folder>\data`.
pub fn data_dirs(folder: &Path, cfg: Option<&scripthook_sdk::config::Value>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    v.push(folder.join("data"));
    if let Some(c) = cfg {
        for d in c.strings_of("data") {
            let p = PathBuf::from(&d);
            v.push(if p.is_absolute() { p } else { folder.join(p) });
        }
    }
    for var in ["SRT_TRAINER_DATA", "SHK_TRAINER_DATA"] {
        if let Ok(s) = std::env::var(var) {
            v.extend(s.split(';').filter(|x| !x.trim().is_empty()).map(PathBuf::from));
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for d in v {
        if d.is_dir() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// `features.tsv` of the trainer data: feature row → details for the description panel.
pub fn data_rows(dirs: &[PathBuf]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for d in dirs {
        let Ok(t) = std::fs::read_to_string(d.join("features.tsv")) else { continue };
        let mut cols: Option<Vec<String>> = None;
        for l in t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
            let f: Vec<&str> = l.split('\t').collect();
            let Some(c) = &cols else {
                cols = Some(f.iter().map(|x| x.trim().to_ascii_lowercase()).collect());
                continue;
            };
            let get = |n: &str| c.iter().position(|x| x == n).and_then(|i| f.get(i)).map(|x| x.trim()).filter(|x| !x.is_empty() && *x != "-");
            let Some(name) = get("feature") else { continue };
            let mut parts = Vec::new();
            if let Some(x) = get("confidence") {
                parts.push(format!("confidence: {x}"));
            }
            if let Some(x) = get("notes") {
                parts.push(x.to_string());
            }
            m.insert(name.to_string(), parts.join("; "));
        }
    }
    m
}

static AUTO: Mutex<Option<Auto>> = Mutex::new(None);

impl shk::Plugin for TrainerPlugin {
    fn load(host: Host) -> Result<Self, String> {
        let folder = host.plugin_folder();
        let cfg = std::fs::read_to_string(folder.join("trainer.toml")).ok().map(|t| toml::parse(&t)).transpose().map_err(|e| format!("trainer.toml: {e}"))?;
        let game = cfg.as_ref().and_then(|c| c.str_of("game"));
        let ident = host.title_identity();
        if let Some(g) = &game {
            if !g.is_empty() && *g != ident {
                host.warn(&format!("trainer: made for {g}, running on {ident} (features may not apply)"));
            }
        }
        // definitions: the shipped ones, then the plugin folder's features\*.toml, then the
        // trainer data's trainer.toml (each replaces features by id)
        let mut defs = features::shipped();
        let dirs = data_dirs(&folder, cfg.as_ref());
        let mut sources: Vec<PathBuf> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(folder.join("features")) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false)).collect();
            v.sort();
            sources.extend(v);
        }
        sources.extend(dirs.iter().map(|d| d.join("trainer.toml")).filter(|p| p.is_file()));
        for p in &sources {
            match std::fs::read_to_string(p) {
                Ok(t) => defs.add(&t, &p.display().to_string()),
                Err(e) => host.warn(&format!("trainer: {}: {e}", p.display())),
            }
        }
        let mut cheats = CheatDefs::default();
        cheats.add(features::SHIPPED_CHEATS, "cheats.toml (shipped)");
        for p in [folder.join("cheats.toml")].into_iter().chain(dirs.iter().map(|d| d.join("cheats.toml"))) {
            if let Ok(t) = std::fs::read_to_string(&p) {
                cheats.add(&t, &p.display().to_string());
            }
        }
        for e in defs.errors.iter().chain(&cheats.errors) {
            host.warn(&format!("trainer: {e}"));
        }
        let (mut lists, errs) = crate::lists::load(&dirs);
        for e in errs {
            host.warn(&format!("trainer: list {e}"));
        }
        // saved locations join the locations list
        let saved = folder.join("saved-locations.toml");
        if saved.is_file() {
            if let Ok(v) = crate::lists::read_list(&saved, "locations") {
                lists.entry("locations".into()).or_default().extend(v);
            }
        }
        let rows = data_rows(&dirs);
        // automation
        let auto = std::env::var("SHK_TRAINER_SCRIPT").ok().map(PathBuf::from).or_else(|| Some(folder.join("autorun.txt")).filter(|p| p.is_file()));
        if let Some(p) = auto {
            match std::fs::read_to_string(&p) {
                Ok(t) => *AUTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(Auto::parse(&t)),
                Err(e) => host.warn(&format!("trainer: {}: {e}", p.display())),
            }
        }
        // the trainer itself is made at the first frame: the host activates its natives database
        // after the plugins are loaded, and every other plugin (the menu) is loaded by then
        let slot: Arc<OnceLock<Arc<Trainer>>> = Arc::new(OnceLock::new());
        let s2 = slot.clone();
        let mut prepared = Some((defs, cheats, lists, rows, folder, dirs));
        let mut last_save = 0.0;
        host.on_frame(move |info| {
            if let Some((defs, cheats, lists, rows, folder, dirs)) = prepared.take() {
                let h = shk::host();
                let tr = Arc::new(Trainer::new(defs, cheats, lists, rows, folder, h.has_natives()));
                h.info(&format!("trainer: {} (data: {})", tr.summary(), if dirs.is_empty() { "none".into() } else { dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("; ") }));
                for (i, a) in tr.avail.iter().enumerate() {
                    if let Some(r) = &a.reason {
                        h.log(scripthook_sdk::ffi::SHK_LOG_DEBUG, &format!("trainer: {} unavailable: {r}", tr.feature(i).id));
                    }
                }
                hotkeys(&tr);
                start(&tr);
                let _ = s2.set(tr);
            }
            let Some(tr) = s2.get() else { return };
            tr.frame(info);
            if let Some(a) = AUTO.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                for l in a.step(tr) {
                    shk::host().info(&l);
                }
            }
            let now = scripthook_sdk::win::now();
            if tr.dirty.load(Ordering::Relaxed) && now - last_save > 1.0 {
                last_save = now;
                tr.dirty.store(false, Ordering::Relaxed);
                if let Err(e) = tr.save_state() {
                    shk::host().warn(&format!("trainer: {}: {e}", tr.state_path.display()));
                }
            }
        });
        Ok(TrainerPlugin { slot })
    }

    fn unload(&mut self) {
        let Some(tr) = self.slot.get() else { return };
        if tr.dirty.load(Ordering::Relaxed) {
            let _ = tr.save_state();
        }
        let m = tr.menu.lock().unwrap_or_else(|e| e.into_inner()).clone();
        m.remove_all();
        let h = shk::host();
        h.info(&format!(
            "trainer: unloaded after {} script ticks, {} runs, {} native calls ({} failed), {} errors",
            tr.stats.ticks.load(Ordering::Relaxed),
            tr.stats.runs.load(Ordering::Relaxed),
            tr.stats.native_calls.load(Ordering::Relaxed),
            tr.stats.native_failures.load(Ordering::Relaxed),
            tr.stats.errors.load(Ordering::Relaxed)
        ));
    }
}

fn hotkeys(tr: &Arc<Trainer>) {
    for i in 0..tr.defs.features.len() {
        if let Some((vk, mods)) = tr.feature(i).hotkey {
            let t2 = tr.clone();
            shk::host().hotkey(vk, mods, move || {
                let f = t2.feature(i);
                let r = if f.kind == crate::features::FKind::Toggle { t2.set(i, !t2.is_on(i)).map(|_| format!("{}: {}", f.name, if t2.is_on(i) { "on" } else { "off" })) } else { t2.run(i, None).map(|_| f.name.clone()) };
                match r {
                    Ok(m) => t2.note(&m),
                    Err(e) => t2.note(&e),
                }
            });
        }
    }
}

/// First frame: every plugin is loaded, so the menu service (if any) can be found.
fn start(tr: &Arc<Trainer>) {
    let h = shk::host();
    let menu = Menu::connect("");
    let kind = match &menu {
        Menu::Service(_) => "the scripthook menu service",
        Menu::Basic(_) => "the host's basic menu",
        Menu::None => "no menu",
    };
    *tr.menu.lock().unwrap_or_else(|e| e.into_inner()) = menu.clone();
    ui::build(tr, &menu);
    let n = tr.load_state();
    let scripts = tr.start_main();
    h.info(&format!("trainer: pages in {kind}; {n} feature(s) on from the saved state; toggle script {}", if scripts { "running" } else { "not needed" }));
}

shk::export_plugin!(TrainerPlugin, name = "gta-vi-scripthook trainer", version = "0.1.0", author = "The gta-vi-scripthook contributors", description = "44 data-driven trainer features on natives and memory cheats.");
