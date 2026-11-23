//! Feature definitions (`[[feature]]`, `[vars]`) and their analysis: which natives, lists,
//! variables and memory locations each feature needs.

use crate::script::{self, Expr, Stmt, Uses};
use scripthook_sdk::config::{toml, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FKind {
    Toggle,
    Action,
    Value,
    List,
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub id: String,
    pub name: String,
    pub group: String,
    pub kind: FKind,
    pub data_feature: String,
    pub note: String,
    pub on: Vec<Stmt>,
    pub tick: Vec<Stmt>,
    pub off: Vec<Stmt>,
    pub run: Vec<Stmt>,
    pub slider: Option<(f64, f64, f64)>,
    pub value: f64,
    pub list: Option<String>,
    pub input: bool,
    pub hotkey: Option<(u32, u32)>,
    pub enabled: bool,
    pub cheat: Option<String>,
    /// statements that did not parse
    pub error: Option<String>,
    pub source: String,
}

impl Feature {
    pub fn all_stmts(&self) -> impl Iterator<Item = &Stmt> {
        self.on.iter().chain(&self.tick).chain(&self.off).chain(&self.run)
    }
    pub fn has_statements(&self) -> bool {
        self.all_stmts().next().is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Defs {
    pub vars: Vec<(String, Expr)>,
    pub features: Vec<Feature>,
    pub errors: Vec<String>,
}

fn stmts(v: &Value, key: &str, err: &mut Option<String>) -> Vec<Stmt> {
    let mut out = Vec::new();
    for s in v.strings_of(key) {
        match script::parse(&s) {
            Ok(mut x) => out.append(&mut x),
            Err(e) => {
                if err.is_none() {
                    *err = Some(format!("{key}: \"{s}\": {e}"));
                }
            }
        }
    }
    out
}

impl Defs {
    /// Adds a file's `[vars]` and `[[feature]]`s; a feature replaces an earlier one of the same id.
    pub fn add(&mut self, text: &str, source: &str) {
        let v = match toml::parse(text) {
            Ok(v) => v,
            Err(e) => {
                self.errors.push(format!("{source}: {e}"));
                return;
            }
        };
        if let Some(t) = v.get("vars").and_then(Value::as_table) {
            for (k, x) in t {
                let Some(s) = x.as_str() else { continue };
                match script::parse_expr(s) {
                    Ok(e) => {
                        self.vars.retain(|(n, _)| n != k);
                        self.vars.push((k.clone(), e));
                    }
                    Err(e) => self.errors.push(format!("{source}: vars.{k}: {e}")),
                }
            }
        }
        for f in v.get("feature").and_then(Value::as_array).unwrap_or(&[]) {
            let Some(id) = f.str_of("id") else {
                self.errors.push(format!("{source}: a feature without id"));
                continue;
            };
            let kind = match f.str_of("kind").as_deref().unwrap_or("toggle") {
                "toggle" => FKind::Toggle,
                "action" => FKind::Action,
                "value" => FKind::Value,
                "list" => FKind::List,
                k => {
                    self.errors.push(format!("{source}: {id}: kind '{k}' (toggle, action, value or list)"));
                    continue;
                }
            };
            let mut error = None;
            let slider = f.get("slider").map(|s| (s.f64_of("min").unwrap_or(0.0), s.f64_of("max").unwrap_or(1.0), s.f64_of("step").unwrap_or(1.0)));
            let feat = Feature {
                name: f.str_of("name").unwrap_or_else(|| id.clone()),
                group: f.str_of("group").unwrap_or_else(|| "Misc".into()),
                kind,
                data_feature: f.str_of("data_feature").unwrap_or_default(),
                note: f.str_of("note").unwrap_or_default(),
                on: stmts(f, "on", &mut error),
                tick: stmts(f, "tick", &mut error),
                off: stmts(f, "off", &mut error),
                run: stmts(f, "run", &mut error),
                value: f.f64_of("value").or(slider.map(|s| s.0)).unwrap_or(0.0),
                slider,
                list: f.str_of("list"),
                input: f.bool_of("input").unwrap_or(false),
                hotkey: f.str_of("hotkey").and_then(|h| scripthook_sdk::keys::hotkey(&h)),
                enabled: f.bool_of("enabled").unwrap_or(false),
                cheat: f.str_of("cheat"),
                error,
                source: source.to_string(),
                id: id.clone(),
            };
            match self.features.iter_mut().find(|x| x.id == id) {
                Some(x) => *x = feat,
                None => self.features.push(feat),
            }
        }
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    /// Everything a feature uses, through the global variables it reads.
    pub fn uses(&self, f: &Feature) -> Uses {
        let mut u = Uses::default();
        let all: Vec<Stmt> = f.all_stmts().cloned().collect();
        script::uses(&all, &mut u);
        let mut seen: Vec<String> = Vec::new();
        let mut k = 0;
        while k < u.vars.len() {
            let v = u.vars[k].clone();
            k += 1;
            if seen.contains(&v) {
                continue;
            }
            seen.push(v.clone());
            if let Some((_, e)) = self.vars.iter().find(|(n, _)| *n == v) {
                script::uses_expr(e, &mut u);
            }
        }
        if let Some(l) = &f.list {
            if !u.lists.contains(l) {
                u.lists.push(l.clone());
            }
        }
        u
    }
}

/// The group order of the menu.
pub const GROUPS: [&str; 6] = ["Player", "Teleport", "Vehicle", "Weapons", "World", "Misc"];

/// The shipped definitions (also in the plugin folder's `features\`).
pub const SHIPPED: [(&str, &str); 7] = [
    ("00-vars.toml", include_str!("../features/00-vars.toml")),
    ("10-player.toml", include_str!("../features/10-player.toml")),
    ("20-teleport.toml", include_str!("../features/20-teleport.toml")),
    ("30-vehicle.toml", include_str!("../features/30-vehicle.toml")),
    ("40-weapons.toml", include_str!("../features/40-weapons.toml")),
    ("50-world.toml", include_str!("../features/50-world.toml")),
    ("60-misc.toml", include_str!("../features/60-misc.toml")),
];

pub const SHIPPED_CHEATS: &str = include_str!("../cheats.toml");

pub fn shipped() -> Defs {
    let mut d = Defs::default();
    for (n, t) in SHIPPED {
        d.add(t, n);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_44_features_parse() {
        let d = shipped();
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        assert_eq!(d.features.len(), 44);
        for f in &d.features {
            assert!(f.error.is_none(), "{}: {:?}", f.id, f.error);
            assert!(GROUPS.contains(&f.group.as_str()), "{}: group {}", f.id, f.group);
        }
        let counts: Vec<usize> = GROUPS.iter().map(|g| d.features.iter().filter(|f| f.group == *g).count()).collect();
        assert_eq!(counts, vec![14, 4, 7, 6, 8, 5]);
        let tp = &d.features[d.index("tp_waypoint").unwrap()];
        let u = d.uses(tp);
        assert!(u.natives.contains(&"HUD::GET_FIRST_BLIP_INFO_ID".to_string()));
        assert!(u.natives.contains(&"PED::GET_VEHICLE_PED_IS_IN".to_string()), "through $veh");
        let w = &d.features[d.index("give_weapons").unwrap()];
        assert_eq!(d.uses(w).lists, vec!["weapons"]);
        let fps = &d.features[d.index("show_fps").unwrap()];
        assert!(d.uses(fps).natives.is_empty());
        assert_eq!(d.vars.len(), 5);
    }

    #[test]
    fn overrides_by_id() {
        let mut d = shipped();
        d.add("[[feature]]\nid = \"health\"\nname = \"God\"\ngroup = \"Player\"\ntick = [\"X::Y()\"]\n[[feature]]\nid = \"new\"\nkind = \"action\"\nrun = [\"bad(\"]\n", "mine.toml");
        assert_eq!(d.features.len(), 45);
        assert_eq!(d.features[d.index("health").unwrap()].name, "God");
        assert!(d.features[d.index("new").unwrap()].error.is_some());
    }
}
