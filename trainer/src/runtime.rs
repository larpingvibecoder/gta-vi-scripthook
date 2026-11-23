//! The trainer at run time: feature availability, the script that runs toggles every script
//! tick, one script per action / list pick, memory cheats every frame, the fly controller, HUD
//! lines, saved state.

use crate::cheats::{self, CKind, CheatDefs, CheatState, HostMem};
use crate::features::{Defs, FKind, Feature};
use crate::script::{Env, Globals, Interp, Item, NArg, Value};
use scripthook_sdk::ffi::*;
use scripthook_sdk::menu::{ItemId, Menu, PageId};
use scripthook_sdk::natives::{Native, Type};
use scripthook_sdk::{self as shk, keys, win};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What implements a feature.
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Natives,
    Cheat(usize),
    None,
}

#[derive(Debug, Clone)]
pub struct Avail {
    pub backend: Backend,
    /// why it is not usable (Backend::None)
    pub reason: Option<String>,
    /// details for the menu's description panel
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct FState {
    pub on: bool,
    pub running: bool,
    pub value: f64,
    pub error: Option<String>,
    /// menu items showing the state (toggle, value)
    pub toggle_item: ItemId,
    pub value_item: ItemId,
    /// list page (list features)
    pub list_page: PageId,
    pub hud_slot: Option<u32>,
}

#[derive(Default)]
pub struct Stats {
    pub native_calls: AtomicU64,
    pub native_failures: AtomicU64,
    pub runs: AtomicU64,
    pub errors: AtomicU64,
    pub ticks: AtomicU64,
}

pub struct Trainer {
    pub defs: Defs,
    pub cheats: CheatDefs,
    pub lists: Mutex<BTreeMap<String, Vec<Item>>>,
    /// data_feature → details (trainer data's features.tsv)
    pub data_rows: HashMap<String, String>,
    pub avail: Vec<Avail>,
    /// cheats no feature claims: (cheat index, unusable reason)
    pub own_cheats: Vec<(usize, Option<String>)>,
    pub fstate: Mutex<Vec<FState>>,
    pub cstate: Mutex<Vec<CheatState>>,
    /// cheats whose switch is pending (cheat, on)
    pending: Mutex<Vec<(usize, bool)>>,
    pub menu: Mutex<Menu>,
    pub folder: PathBuf,
    pub state_path: PathBuf,
    pub dirty: AtomicBool,
    fps: AtomicU64,
    frame_times: Mutex<std::collections::VecDeque<f64>>,
    menu_open: AtomicBool,
    pub stats: Stats,
    natives: Mutex<HashMap<String, Native>>,
    pub notes: Mutex<Vec<String>>,
    pub script_mode: String,
    /// menu items of the cheats no feature claims
    pub cheat_items: Mutex<HashMap<usize, ItemId>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Trainer {
    /// Builds the trainer: availability of every feature from the host's natives database, the
    /// lists and the memory cheats.
    pub fn new(defs: Defs, cheats: CheatDefs, lists: BTreeMap<String, Vec<Item>>, data_rows: HashMap<String, String>, folder: PathBuf, natives_ok: bool) -> Trainer {
        let mut natives: HashMap<String, Native> = HashMap::new();
        let mut avail = Vec::new();
        let mut claimed = vec![false; cheats.cheats.len()];
        for f in &defs.features {
            let cheat_id = f.cheat.clone().unwrap_or_else(|| f.id.clone());
            let ci = cheats.cheats.iter().position(|c| c.id == cheat_id);
            if let Some(ci) = ci {
                claimed[ci] = true;
            }
            let u = defs.uses(f);
            let mut reason: Option<String> = f.error.clone().map(|e| format!("statement error: {e}"));
            if reason.is_none() {
                for n in &u.natives {
                    if !natives_ok {
                        reason = Some(format!("needs native {n} (the host has no script hook)"));
                        break;
                    }
                    let nat = natives.entry(n.clone()).or_insert_with(|| Native::lookup(n).unwrap_or(Native { handle: 0, info: Default::default() }));
                    if !nat.info.available {
                        reason = Some(if nat.info.reason.is_empty() { format!("needs native {n} (unknown until the executable)") } else { nat.info.reason.clone() });
                        break;
                    }
                }
            }
            if reason.is_none() {
                for l in &u.lists {
                    let empty = lists.get(l).map(|x| x.is_empty()).unwrap_or(true);
                    if empty && !(f.input && f.list.as_deref() == Some(l.as_str())) {
                        reason = Some(format!("needs the list '{l}' (no {l}.txt / .tsv / .json / .toml in the trainer data)"));
                        break;
                    }
                }
            }
            if reason.is_none() {
                for m in &u.mem {
                    match cheats.location(m) {
                        Some(l) => {
                            if let Err(e) = l.known() {
                                reason = Some(e);
                                break;
                            }
                        }
                        None => {
                            reason = Some(format!("no memory location '{m}'"));
                            break;
                        }
                    }
                }
            }
            let mut detail = String::new();
            if !f.note.is_empty() {
                detail.push_str(&f.note);
            }
            if let Some(d) = data_rows.get(&f.data_feature) {
                if !detail.is_empty() {
                    detail.push_str(" | ");
                }
                detail.push_str(d);
            }
            let a = match reason {
                None => Avail { backend: Backend::Natives, reason: None, detail },
                Some(r) => match ci.map(|c| (c, cheats.cheats[c].unusable())) {
                    Some((c, None)) => Avail { backend: Backend::Cheat(c), reason: None, detail: format!("memory cheat ({}) | {detail}", cheats.cheats[c].kind_name) },
                    Some((_, Some(cr))) => Avail { backend: Backend::None, reason: Some(format!("{r}; {cr}")), detail },
                    None => Avail { backend: Backend::None, reason: Some(r), detail },
                },
            };
            avail.push(a);
        }
        let own_cheats = cheats.cheats.iter().enumerate().filter(|(i, _)| !claimed[*i]).map(|(i, c)| (i, c.unusable())).collect();
        let fstate = defs.features.iter().map(|f| FState { value: f.value, ..Default::default() }).collect();
        let cstate = cheats.cheats.iter().map(|c| CheatState { value: if c.kind == CKind::Multiply { c.factor } else { c.value }, ..Default::default() }).collect();
        let mode = if shk::loaded() { format!("{:?}", shk::host().script_status().mode) } else { "Off".into() };
        Trainer {
            defs,
            cheats,
            lists: Mutex::new(lists),
            data_rows,
            avail,
            own_cheats,
            fstate: Mutex::new(fstate),
            cstate: Mutex::new(cstate),
            pending: Mutex::new(Vec::new()),
            menu: Mutex::new(Menu::None),
            state_path: folder.join("state.toml"),
            folder,
            dirty: AtomicBool::new(false),
            fps: AtomicU64::new(0),
            frame_times: Mutex::new(std::collections::VecDeque::new()),
            menu_open: AtomicBool::new(false),
            stats: Stats::default(),
            natives: Mutex::new(natives),
            notes: Mutex::new(Vec::new()),
            script_mode: mode,
            cheat_items: Mutex::new(HashMap::new()),
        }
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.defs.index(id)
    }

    pub fn feature(&self, i: usize) -> &Feature {
        &self.defs.features[i]
    }

    pub fn usable(&self, i: usize) -> bool {
        self.avail[i].backend != Backend::None
    }

    pub fn usable_count(&self) -> usize {
        self.avail.iter().filter(|a| a.backend != Backend::None).count()
    }

    pub fn is_on(&self, i: usize) -> bool {
        lock(&self.fstate)[i].on
    }

    pub fn value(&self, i: usize) -> f64 {
        lock(&self.fstate)[i].value
    }

    pub fn fps(&self) -> f64 {
        f64::from_bits(self.fps.load(Ordering::Relaxed))
    }

    pub fn menu_open(&self) -> bool {
        self.menu_open.load(Ordering::Relaxed)
    }

    pub fn note(&self, text: &str) {
        let m = lock(&self.menu).clone();
        m.toast(text, 3.0);
        lock(&self.notes).push(text.to_string());
        if shk::loaded() {
            shk::host().info(&format!("trainer: {text}"));
        }
    }

    /// Switches a toggle feature (natives: from the next script tick; cheat: next frame).
    pub fn set(&self, i: usize, on: bool) -> Result<(), String> {
        let f = self.feature(i);
        if !self.usable(i) {
            return Err(self.avail[i].reason.clone().unwrap_or_default());
        }
        if f.kind != FKind::Toggle {
            return Err(format!("{} is not a toggle", f.name));
        }
        let item = {
            let mut st = lock(&self.fstate);
            st[i].on = on;
            st[i].error = None;
            st[i].toggle_item
        };
        if let Backend::Cheat(c) = self.avail[i].backend {
            lock(&self.pending).push((c, on));
        }
        if item != 0 {
            lock(&self.menu).set_value(item, on as i32 as f64);
        }
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Switches a memory cheat that no feature claims.
    pub fn cheat_switch(&self, c: usize, on: bool) {
        lock(&self.pending).push((c, on));
        if let Some(&item) = lock(&self.cheat_items).get(&c) {
            lock(&self.menu).set_value(item, on as i32 as f64);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn cheat_on(&self, c: usize) -> bool {
        lock(&self.cstate)[c].on
    }

    pub fn set_value(&self, i: usize, v: f64) {
        let f = self.feature(i);
        let v = match f.slider {
            Some((lo, hi, _)) => v.clamp(lo, hi),
            None => v,
        };
        let item = {
            let mut st = lock(&self.fstate);
            st[i].value = v;
            st[i].value_item
        };
        if let Backend::Cheat(c) = self.avail[i].backend {
            lock(&self.cstate)[c].value = v;
        }
        if item != 0 {
            lock(&self.menu).set_value(item, v);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Runs an action / value / list feature in a script of its own.
    pub fn run(self: &Arc<Self>, i: usize, item: Option<Item>) -> Result<(), String> {
        if !self.usable(i) {
            return Err(self.avail[i].reason.clone().unwrap_or_default());
        }
        let f = self.feature(i);
        if let Backend::Cheat(c) = self.avail[i].backend {
            // a cheat-backed action writes once
            lock(&self.pending).push((c, true));
            return Ok(());
        }
        if f.run.is_empty() {
            return Err(format!("{} has nothing to run", f.name));
        }
        let tr = self.clone();
        let value = self.value(i);
        let started = shk::host().start_script(&format!("trainer: {}", f.id), move || {
            let f = tr.feature(i);
            let mut g = Globals { defs: tr.defs.vars.iter().cloned().collect(), cache: HashMap::new() };
            let mut it = Interp::new();
            it.set("value", Value::Float(value));
            if let Some(item) = &item {
                it.set_item(item, 0);
            }
            let mut env = ScriptEnv::new(tr.clone(), i);
            tr.stats.runs.fetch_add(1, Ordering::Relaxed);
            if let Err(e) = it.run(&f.run, &mut g, &mut env) {
                tr.stats.errors.fetch_add(1, Ordering::Relaxed);
                lock(&tr.fstate)[i].error = Some(e.clone());
                tr.note(&format!("{}: {e}", f.name));
            }
        });
        started.map(|_| ()).ok_or_else(|| "the host has no script functions".into())
    }

    /// The script that runs the toggles every script tick (started once).
    pub fn start_main(self: &Arc<Self>) -> bool {
        let toggles: Vec<usize> = (0..self.defs.features.len()).filter(|&i| self.feature(i).kind == FKind::Toggle && self.avail[i].backend == Backend::Natives && self.feature(i).has_statements()).collect();
        if toggles.is_empty() {
            return false;
        }
        // HUD slots for features using hud()
        {
            let mut st = lock(&self.fstate);
            let mut slot = 0;
            for &i in &toggles {
                let uses_hud = self.defs.uses(self.feature(i)).builtins.iter().any(|b| b == "hud");
                if uses_hud && slot < 8 {
                    st[i].hud_slot = Some(slot);
                    slot += 1;
                }
            }
        }
        let tr = self.clone();
        shk::host()
            .start_script("trainer: toggles", move || {
                let mut g = Globals { defs: tr.defs.vars.iter().cloned().collect(), cache: HashMap::new() };
                let mut interps: HashMap<usize, Interp> = HashMap::new();
                let mut envs: HashMap<usize, ScriptEnv> = HashMap::new();
                loop {
                    if shk::rt::unloading() {
                        return;
                    }
                    g.new_tick();
                    tr.stats.ticks.fetch_add(1, Ordering::Relaxed);
                    for &i in &toggles {
                        let (want, running, value, slot) = {
                            let st = lock(&tr.fstate);
                            (st[i].on, st[i].running, st[i].value, st[i].hud_slot)
                        };
                        if !want && !running {
                            continue;
                        }
                        let f = tr.feature(i);
                        let it = interps.entry(i).or_default();
                        let env = envs.entry(i).or_insert_with(|| ScriptEnv::new(tr.clone(), i));
                        env.hud = None;
                        it.set("value", Value::Float(value));
                        let r = (|| -> Result<(), String> {
                            if want && !running {
                                it.run(&f.on, &mut g, &mut *env)?;
                                lock(&tr.fstate)[i].running = true;
                            }
                            if want {
                                it.run(&f.tick, &mut g, &mut *env)?;
                            } else if running {
                                lock(&tr.fstate)[i].running = false;
                                it.run(&f.off, &mut g, &mut *env)?;
                            }
                            Ok(())
                        })();
                        if let Some(s) = slot {
                            let text = if want && r.is_ok() { env.hud.clone().unwrap_or_default() } else { String::new() };
                            lock(&tr.menu).set_hud(s, &text);
                        }
                        if let Err(e) = r {
                            tr.stats.errors.fetch_add(1, Ordering::Relaxed);
                            let item = {
                                let mut st = lock(&tr.fstate);
                                st[i].on = false;
                                st[i].running = false;
                                st[i].error = Some(e.clone());
                                st[i].toggle_item
                            };
                            if item != 0 {
                                lock(&tr.menu).set_value(item, 0.0);
                            }
                            tr.note(&format!("{} switched off: {e}", f.name));
                        }
                    }
                    if !shk::wait(0) {
                        return;
                    }
                }
            })
            .is_some()
    }

    /// Frame work: FPS, the menu state, memory cheats.
    pub fn frame(&self, info: &ShkFrameInfo) {
        {
            // presented frames in the last second
            let now = win::now();
            let mut w = lock(&self.frame_times);
            w.push_back(now);
            while w.front().map(|t| now - t > 1.0).unwrap_or(false) {
                w.pop_front();
            }
            let span = now - w.front().copied().unwrap_or(now);
            let v = if span > 0.25 { (w.len() - 1) as f64 / span } else { 0.0 };
            self.fps.store(v.to_bits(), Ordering::Relaxed);
        }
        let _ = info.dt;
        let mopen = info.menu_open != 0 || lock(&self.menu).is_open();
        self.menu_open.store(mopen, Ordering::Relaxed);
        let pending: Vec<(usize, bool)> = std::mem::take(&mut *lock(&self.pending));
        let mut cs = lock(&self.cstate);
        for (c, on) in pending {
            let ch = &self.cheats.cheats[c];
            cs[c].on = on && ch.kind != CKind::Set;
            cheats::apply(ch, &mut cs[c], &HostMem, Some(on));
        }
        for (c, ch) in self.cheats.cheats.iter().enumerate() {
            if cs[c].on {
                cheats::apply(ch, &mut cs[c], &HostMem, None);
            }
        }
    }

    // ---- state file ----------------------------------------------------------------------------

    pub fn load_state(&self) -> usize {
        let Ok(t) = std::fs::read_to_string(&self.state_path) else { return 0 };
        let Ok(v) = scripthook_sdk::config::toml::parse(&t) else {
            shk::host().warn(&format!("trainer: {} is not valid TOML; ignored", self.state_path.display()));
            return 0;
        };
        let mut n = 0;
        if let Some(vals) = v.get("value").and_then(|x| x.as_table()) {
            for (k, x) in vals {
                if let (Some(i), Some(f)) = (self.index(k), x.as_f64()) {
                    self.set_value(i, f);
                }
            }
        }
        if let Some(on) = v.get("on").and_then(|x| x.as_table()) {
            for (k, x) in on {
                if let (Some(i), Some(true)) = (self.index(k), x.as_bool()) {
                    if self.set(i, true).is_ok() {
                        n += 1;
                    }
                }
            }
        }
        // features marked enabled = true in their definition start on
        for i in 0..self.defs.features.len() {
            if self.feature(i).enabled && self.feature(i).kind == FKind::Toggle && !self.is_on(i) && !v.path(&format!("on.{}", self.feature(i).id)).is_some() {
                let _ = self.set(i, true);
            }
        }
        self.dirty.store(false, Ordering::Relaxed);
        n
    }

    pub fn save_state(&self) -> std::io::Result<()> {
        let st = lock(&self.fstate);
        let mut t = String::from("# gta-vi-scripthook trainer: saved state (written by the trainer)\n\n[on]\n");
        for (i, f) in self.defs.features.iter().enumerate() {
            if f.kind == FKind::Toggle && st[i].on {
                t.push_str(&format!("{} = true\n", f.id));
            }
        }
        t.push_str("\n[value]\n");
        for (i, f) in self.defs.features.iter().enumerate() {
            if f.slider.is_some() && st[i].value != f.value {
                t.push_str(&format!("{} = {}\n", f.id, st[i].value));
            }
        }
        drop(st);
        std::fs::write(&self.state_path, t)
    }

    /// A one-line summary for the log and the menu.
    pub fn summary(&self) -> String {
        let lists = lock(&self.lists);
        let n_lists: Vec<String> = lists.iter().map(|(k, v)| format!("{k} {}", v.len())).collect();
        format!(
            "{} features, {} usable ({} natives, {} memory cheats); {} memory cheats of their own; lists: {}",
            self.defs.features.len(),
            self.usable_count(),
            self.avail.iter().filter(|a| a.backend == Backend::Natives).count(),
            self.avail.iter().filter(|a| matches!(a.backend, Backend::Cheat(_))).count(),
            self.own_cheats.len(),
            if n_lists.is_empty() { "none".into() } else { n_lists.join(", ") }
        )
    }

    fn native(&self, name: &str) -> Result<Native, String> {
        if let Some(n) = lock(&self.natives).get(name) {
            return Ok(n.clone());
        }
        let n = Native::lookup(name).map_err(|e| e.to_string())?;
        lock(&self.natives).insert(name.to_string(), n.clone());
        Ok(n)
    }

    pub fn save_location(&self, pos: [f64; 3], heading: f64) -> Result<String, String> {
        let path = self.folder.join("saved-locations.toml");
        let n = lock(&self.lists).get("locations").map(|l| l.iter().filter(|x| x.name.starts_with("Saved ")).count()).unwrap_or(0) + 1;
        let name = format!("Saved {n}");
        let entry = format!("\n[[locations]]\nname = \"{name}\"\npos = [{:.3}, {:.3}, {:.3}]\nheading = {heading:.2}\n", pos[0], pos[1], pos[2]);
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| f.write_all(entry.as_bytes())).map_err(|e| format!("{}: {e}", path.display()))?;
        let item = Item { name: name.clone(), label: name.clone(), hash: None, pos: Some(pos), heading: Some(heading) };
        lock(&self.lists).entry("locations".into()).or_default().push(item.clone());
        Ok(name)
    }
}

// ---------------------------------------------------------------------------------------------
// The environment statements run in (a script fiber)

pub struct ScriptEnv {
    tr: Arc<Trainer>,
    feature: usize,
    pub hud: Option<String>,
    last_fly: f64,
}

impl ScriptEnv {
    pub fn new(tr: Arc<Trainer>, feature: usize) -> ScriptEnv {
        ScriptEnv { tr, feature, hud: None, last_fly: 0.0 }
    }

    fn fly(&mut self, pos: [f64; 3], heading_deg: f64) -> [f64; 3] {
        let now = win::now();
        let dt = if self.last_fly == 0.0 { 0.0 } else { (now - self.last_fly).clamp(0.0, 0.1) };
        self.last_fly = now;
        if self.tr.menu_open() {
            return pos;
        }
        let h = shk::host();
        let k = |vk: u32| h.key_down(vk) as i32 as f64;
        let pad = h.pad(0).unwrap_or_default();
        let dz = |v: f32| if v.abs() < 0.2 { 0.0 } else { v as f64 };
        let f = (k(b'W' as u32) - k(b'S' as u32) + dz(pad.left_y)).clamp(-1.0, 1.0);
        let r = (k(b'D' as u32) - k(b'A' as u32) + dz(pad.left_x)).clamp(-1.0, 1.0);
        let u = (k(keys::VK_SPACE) - k(keys::VK_CONTROL) + dz(pad.right_trigger) - dz(pad.left_trigger)).clamp(-1.0, 1.0);
        let fast = if k(keys::VK_SHIFT) > 0.0 || pad.buttons & SHK_PAD_LS != 0 { 3.0 } else { 1.0 };
        let speed = self.tr.value(self.feature).max(0.1) * fast * dt;
        let a = heading_deg.to_radians();
        let (fx, fy) = (-a.sin(), a.cos());
        let (rx, ry) = (a.cos(), a.sin());
        [pos[0] + (fx * f + rx * r) * speed, pos[1] + (fy * f + ry * r) * speed, pos[2] + u * speed]
    }
}

fn to_arg(v: &Value, declared: Option<Type>, keep: &mut Vec<std::ffi::CString>) -> ShkValue {
    let mut x = ShkValue::default();
    match (v, declared) {
        (Value::Float(f), Some(Type::Int | Type::Hash | Type::Bool)) => {
            x.ty = SHK_TYPE_INT;
            x.u[0] = (*f as i64) as u32 as u64;
        }
        (Value::Int(i), Some(Type::Float)) => {
            x.ty = SHK_TYPE_FLOAT;
            x.u[0] = (*i as f32).to_bits() as u64;
        }
        (Value::Int(i), Some(Type::Hash)) => {
            x.ty = SHK_TYPE_HASH;
            x.u[0] = *i as u32 as u64;
        }
        (Value::Int(i), _) => {
            x.ty = SHK_TYPE_INT;
            x.u[0] = *i as u32 as u64;
        }
        (Value::Float(f), _) => {
            x.ty = SHK_TYPE_FLOAT;
            x.u[0] = (*f as f32).to_bits() as u64;
        }
        (Value::Bool(b), _) => {
            x.ty = SHK_TYPE_BOOL;
            x.u[0] = *b as u64;
        }
        (Value::Str(s), _) => {
            let c = std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default();
            x.ty = SHK_TYPE_STRING;
            x.u[0] = c.as_ptr() as u64;
            keep.push(c);
        }
        (Value::Vec3(a), _) => {
            x.ty = SHK_TYPE_VEC3;
            x.u[0] = (a[0] as f32).to_bits() as u64 | ((a[1] as f32).to_bits() as u64) << 32;
            x.u[1] = (a[2] as f32).to_bits() as u64;
        }
        (Value::Nil, _) => x.ty = SHK_TYPE_INT,
    }
    x
}

fn from_ret(r: &ShkValue) -> Value {
    match Type::from_code(r.ty) {
        Some(Type::Void) => Value::Nil,
        Some(Type::Float) => Value::Float(f32::from_bits(r.u[0] as u32) as f64),
        Some(Type::Bool) => Value::Bool(r.u[0] as u32 != 0),
        Some(Type::Hash) => Value::Int(r.u[0] as u32 as i64),
        Some(Type::Vec3) => Value::Vec3([f32::from_bits(r.u[0] as u32) as f64, f32::from_bits((r.u[0] >> 32) as u32) as f64, f32::from_bits(r.u[1] as u32) as f64]),
        Some(Type::Str) => Value::Str(if r.u[0] == 0 { String::new() } else { unsafe { std::ffi::CStr::from_ptr(r.u[0] as *const std::ffi::c_char) }.to_string_lossy().into_owned() }),
        Some(Type::U64 | Type::Ptr) => Value::Int(r.u[0] as i64),
        _ => Value::Int(r.u[0] as u32 as i32 as i64),
    }
}

impl Env for ScriptEnv {
    fn native(&mut self, name: &str, args: &[NArg]) -> Result<(Value, Vec<Value>), String> {
        let n = self.tr.native(name)?;
        if !n.info.available {
            return Err(if n.info.reason.is_empty() { format!("needs native {name}") } else { n.info.reason.clone() });
        }
        let decl = n.info.args.clone();
        let mut keep = Vec::new();
        // out storage: 16 bytes each, stable during the call
        let mut outs: Vec<Box<[u8; 16]>> = Vec::new();
        let mut out_types: Vec<Type> = Vec::new();
        let mut vals = Vec::with_capacity(args.len());
        let mut cursor = 0usize;
        for a in args {
            let d = decl.as_ref().and_then(|v| v.get(cursor).copied());
            match a {
                NArg::Val(v) => {
                    vals.push(to_arg(v, d, &mut keep));
                    cursor += if matches!(v, Value::Vec3(_)) && d != Some(Type::Vec3) { 3 } else { 1 };
                }
                NArg::Out(cur) => {
                    let t = match d {
                        Some(t) if t.is_out() => t,
                        _ => match cur {
                            Value::Float(_) => Type::OutFloat,
                            Value::Vec3(_) => Type::OutVec3,
                            Value::Bool(_) => Type::OutBool,
                            _ => Type::OutInt,
                        },
                    };
                    let mut b = Box::new([0u8; 16]);
                    match (t, cur) {
                        (Type::OutFloat, v) => b[..4].copy_from_slice(&(v.as_f64().unwrap_or(0.0) as f32).to_le_bytes()),
                        (Type::OutVec3, Value::Vec3(a)) => {
                            for k in 0..3 {
                                b[k * 4..k * 4 + 4].copy_from_slice(&(a[k] as f32).to_le_bytes());
                            }
                        }
                        (_, v) => b[..4].copy_from_slice(&(v.as_i64().unwrap_or(0) as i32).to_le_bytes()),
                    }
                    vals.push(ShkValue { ty: t.code(), reserved: 0, u: [b.as_ptr() as u64, 0] });
                    outs.push(b);
                    out_types.push(t);
                    cursor += 1;
                }
            }
        }
        let ret = n.info.ret.map(|t| t.code()).unwrap_or(SHK_TYPE_INT);
        self.tr.stats.native_calls.fetch_add(1, Ordering::Relaxed);
        let r = n.call_raw(&vals, ret).map_err(|e| {
            self.tr.stats.native_failures.fetch_add(1, Ordering::Relaxed);
            e.to_string()
        })?;
        drop(keep);
        let out_vals = outs
            .iter()
            .zip(&out_types)
            .map(|(b, t)| {
                let f = |k: usize| f32::from_le_bytes(b[k..k + 4].try_into().unwrap()) as f64;
                match t {
                    Type::OutFloat => Value::Float(f(0)),
                    Type::OutVec3 => Value::Vec3([f(0), f(4), f(8)]),
                    Type::OutBool => Value::Bool(i32::from_le_bytes(b[..4].try_into().unwrap()) != 0),
                    _ => Value::Int(i32::from_le_bytes(b[..4].try_into().unwrap()) as i64),
                }
            })
            .collect();
        Ok((if ret == SHK_TYPE_VOID { Value::Nil } else { from_ret(&r) }, out_vals))
    }

    fn wait(&mut self, ms: u32) -> Result<(), String> {
        if shk::rt::unloading() {
            return Err("the trainer is unloading".into());
        }
        if shk::wait(ms) {
            Ok(())
        } else {
            Err("wait outside a script function".into())
        }
    }

    fn builtin(&mut self, name: &str, a: &[Value]) -> Option<Result<Value, String>> {
        let txt = |k: usize| a.get(k).map(|v| v.text()).unwrap_or_default();
        Some(match name {
            "notify" => {
                self.tr.note(&txt(0));
                Ok(Value::Nil)
            }
            "hud" => {
                self.hud = Some(txt(0));
                Ok(Value::Nil)
            }
            "fps" => Ok(Value::Float(self.tr.fps())),
            "on" => Ok(Value::Bool(self.tr.index(&txt(0)).map(|i| self.tr.is_on(i)).unwrap_or(false))),
            "value" => Ok(self.tr.index(&txt(0)).map(|i| Value::Float(self.tr.value(i))).unwrap_or(Value::Nil)),
            "fly" => match (a.first(), a.get(1).and_then(|v| v.as_f64())) {
                (Some(Value::Vec3(p)), Some(h)) => Ok(Value::Vec3(self.fly(*p, h))),
                _ => Err("fly(position, heading in degrees)".into()),
            },
            "mem" | "setmem" => {
                let id = txt(0);
                let Some(loc) = self.tr.cheats.location(&id) else { return Some(Err(format!("no memory location '{id}'"))) };
                let addr = match loc.resolve(&HostMem) {
                    Ok(x) => x,
                    Err(e) => return Some(Err(e)),
                };
                if name == "mem" {
                    let mut b = vec![0u8; loc.ty.size()];
                    if shk::host().read_bytes(addr, &mut b) {
                        Ok(loc.ty.decode(&b))
                    } else {
                        Err(format!("{id}: unreadable at {addr:#x}"))
                    }
                } else {
                    match a.get(1).and_then(|v| loc.ty.encode(v)) {
                        Some(b) if shk::host().write_bytes(addr, &b) => Ok(Value::Nil),
                        _ => Err(format!("{id}: write failed at {addr:#x}")),
                    }
                }
            }
            "save_location" => match (a.first(), a.get(1).and_then(|v| v.as_f64())) {
                (Some(Value::Vec3(p)), h) => self.tr.save_location(*p, h.unwrap_or(0.0)).map(|n| {
                    self.tr.note(&format!("location saved as {n}"));
                    Value::Nil
                }),
                _ => Err("save_location(position, heading)".into()),
            },
            _ => return None,
        })
    }

    fn list(&mut self, name: &str) -> Option<Vec<Item>> {
        lock(&self.tr.lists).get(name).cloned()
    }
}
