//! `natives.tsv`: reading, checking and writing (docs/natives-database.md).

use scripthook_sdk::config::{toml, Value};
use scripthook_sdk::natives::Type;

/// A native's identifier in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id {
    Hash(u64),
    Index(u32),
}

impl Id {
    pub fn parse(s: &str) -> Result<Id, String> {
        let t = s.trim();
        if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            return u64::from_str_radix(h, 16).map(Id::Hash).map_err(|_| format!("identifier '{t}': bad hex"));
        }
        t.parse::<u32>().map(Id::Index).map_err(|_| format!("identifier '{t}': a 0x hash or a decimal index"))
    }
    pub fn text(&self) -> String {
        match self {
            Id::Hash(h) => format!("{h:#018x}"),
            Id::Index(i) => i.to_string(),
        }
    }
}

/// Declared arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum Args {
    Unknown,
    /// a slot count only
    Slots(usize),
    /// (spelling as written, parsed type)
    Types(Vec<(String, Type)>),
}

/// Declared result.
#[derive(Debug, Clone, PartialEq)]
pub enum Ret {
    Unknown,
    Type(String, Type),
}

#[derive(Debug, Clone)]
pub struct Native {
    /// `NS::NAME` (or `NAME`)
    pub name: String,
    pub id: Option<Id>,
    pub args: Args,
    pub ret: Ret,
    /// parameter names (optional `params` column)
    pub params: Vec<String>,
    /// free text (optional `doc` / `notes` column)
    pub doc: String,
    /// every column as written (for round trips)
    pub cols: Vec<(String, String)>,
    pub line: usize,
}

impl Native {
    pub fn namespace(&self) -> &str {
        self.name.split_once("::").map(|x| x.0).unwrap_or("")
    }
    pub fn short(&self) -> &str {
        self.name.split_once("::").map(|x| x.1).unwrap_or(&self.name)
    }
}

/// `@native_table`, `@native_context`, `@script_tick` rows.
#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    /// the inline TOML table as written
    pub body: String,
    pub value: Value,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Db {
    pub columns: Vec<String>,
    pub directives: Vec<Directive>,
    pub natives: Vec<Native>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub const DIRECTIVES: [&str; 3] = ["native_table", "native_context", "script_tick"];

/// Parses `natives.tsv`. Errors are collected per line; rows with errors are skipped.
pub fn parse(text: &str) -> Db {
    let mut db = Db::default();
    for (ln0, l) in text.lines().enumerate() {
        let ln = ln0 + 1;
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').map(str::trim).collect();
        if let Some(k) = f[0].strip_prefix('@') {
            let body = f[1..].iter().copied().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ");
            if !DIRECTIVES.contains(&k) {
                db.warnings.push(format!("line {ln}: unknown directive @{k} (ignored by hosts)"));
            }
            match toml::parse(&format!("v = {body}")) {
                Ok(v) => db.directives.push(Directive { name: k.to_string(), body, value: v.get("v").cloned().unwrap_or(Value::Null), line: ln }),
                Err(e) => db.errors.push(format!("line {ln}: @{k}: {e}")),
            }
            continue;
        }
        if db.columns.is_empty() {
            db.columns = f.iter().map(|x| x.to_ascii_lowercase()).collect();
            if !db.columns.iter().any(|c| c == "name") {
                db.errors.push(format!("line {ln}: the header row has no 'name' column"));
            }
            continue;
        }
        let get = |n: &str| db.columns.iter().position(|x| x == n).and_then(|i| f.get(i)).copied().filter(|x| !x.is_empty() && *x != "-");
        let Some(name) = get("name") else {
            db.errors.push(format!("line {ln}: a native without a name"));
            continue;
        };
        let id = match get("identifier").or(get("hash")).or(get("index")) {
            None => None,
            Some(x) => match Id::parse(x) {
                Ok(i) => Some(i),
                Err(e) => {
                    db.errors.push(format!("line {ln}: {name}: {e}"));
                    continue;
                }
            },
        };
        let args = match get("args") {
            None => Args::Unknown,
            Some(a) => match a.parse::<usize>() {
                Ok(n) => Args::Slots(n),
                Err(_) => {
                    let mut v = Vec::new();
                    let mut bad = None;
                    for t in a.split([',', ';']).map(str::trim).filter(|x| !x.is_empty()) {
                        match Type::parse(t) {
                            Some(Type::Void) => bad = Some(format!("'void' is not an argument type in '{a}'")),
                            Some(ty) => v.push((t.to_string(), ty)),
                            None => bad = Some(format!("unknown argument type '{t}'")),
                        }
                    }
                    if let Some(b) = bad {
                        db.errors.push(format!("line {ln}: {name}: {b}"));
                        continue;
                    }
                    Args::Types(v)
                }
            },
        };
        let ret = match get("returns") {
            None | Some("1") => Ret::Unknown,
            Some("0") => Ret::Type("void".into(), Type::Void),
            Some("3") => Ret::Type("Vector3".into(), Type::Vec3),
            Some(r) => match Type::parse(r) {
                Some(t) if !t.is_out() => Ret::Type(r.to_string(), t),
                _ => {
                    db.errors.push(format!("line {ln}: {name}: unknown result type '{r}'"));
                    continue;
                }
            },
        };
        let params: Vec<String> = get("params").map(|p| p.split([',', ';']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
        if let (Args::Types(t), false) = (&args, params.is_empty()) {
            if t.len() != params.len() {
                db.warnings.push(format!("line {ln}: {name}: {} parameter names for {} arguments", params.len(), t.len()));
            }
        }
        let doc = get("doc").or(get("notes")).unwrap_or("").to_string();
        let cols = db.columns.iter().enumerate().map(|(i, c)| (c.clone(), f.get(i).copied().unwrap_or("").to_string())).collect();
        db.natives.push(Native { name: name.to_string(), id, args, ret, params, doc, cols, line: ln });
    }
    db
}

impl Db {
    /// Consistency checks beyond parsing: duplicate names and identifiers, directive keys.
    pub fn check(&mut self) {
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut ids: std::collections::HashMap<String, (usize, String)> = std::collections::HashMap::new();
        for n in &self.natives {
            if let Some(prev) = seen.insert(n.name.to_ascii_uppercase(), n.line) {
                self.errors.push(format!("line {}: {} is also on line {prev}", n.line, n.name));
            }
            if !valid_name(&n.name) {
                self.errors.push(format!("line {}: '{}' is not NS::NAME or NAME (letters, digits, '_')", n.line, n.name));
            }
            if let Some(id) = n.id {
                if let Some((l, other)) = ids.insert(id.text(), (n.line, n.name.clone())) {
                    self.errors.push(format!("line {}: {} has the identifier of {other} (line {l})", n.line, n.name));
                }
            }
        }
        let kinds: Vec<Id> = self.natives.iter().filter_map(|n| n.id).collect();
        if kinds.iter().any(|i| matches!(i, Id::Hash(_))) && kinds.iter().any(|i| matches!(i, Id::Index(_))) {
            self.errors.push("identifiers mix hashes and indices: a handler table is keyed by one or the other".into());
        }
        for d in &self.directives {
            let known: &[&str] = match d.name.as_str() {
                "native_table" => &["kind", "buckets", "bucket_index", "next", "handlers", "capacity", "count", "count_type", "hashes", "hash_xor", "stride", "hash", "handler", "hash_type", "module", "section", "pattern", "nth", "rip", "offset", "export", "address", "chain"],
                "native_context" => &["size", "return_ptr", "arg_count", "args_ptr", "data_count", "fixup_out", "fixup_in", "fixups", "slot"],
                "script_tick" => &["module", "section", "pattern", "nth", "rip", "offset", "export", "address", "chain"],
                _ => continue,
            };
            match d.value.as_table() {
                Some(t) => {
                    for (k, _) in t {
                        if !known.contains(&k.as_str()) {
                            self.warnings.push(format!("line {}: @{}: unknown key '{k}'", d.line, d.name));
                        }
                    }
                    if d.name == "native_table" {
                        if let Some(k) = d.value.str_of("kind") {
                            if !["buckets", "pairs", "array", "index"].contains(&k.as_str()) {
                                self.errors.push(format!("line {}: @native_table: kind '{k}' (buckets, pairs or array)", d.line));
                            }
                        }
                    }
                }
                None => self.errors.push(format!("line {}: @{}: expected an inline table {{ ... }}", d.line, d.name)),
            }
        }
    }

    pub fn find(&self, name: &str) -> Option<&Native> {
        let u = name.to_ascii_uppercase();
        self.natives.iter().find(|n| n.name.to_ascii_uppercase() == u).or_else(|| self.natives.iter().find(|n| n.short().to_ascii_uppercase() == u))
    }

    /// Canonical text (header, directives, rows sorted by namespace and name).
    pub fn to_tsv(&self) -> String {
        let mut t = String::from("# natives database (docs/natives-database.md)\nidentifier\targs\treturns\tname\tparams\tdoc\n");
        for d in &self.directives {
            t.push_str(&format!("@{}\t{}\n", d.name, d.body));
        }
        let mut ns: Vec<&Native> = self.natives.iter().collect();
        ns.sort_by(|a, b| (a.namespace(), a.short()).cmp(&(b.namespace(), b.short())));
        for n in ns {
            let id = n.id.map(|i| i.text()).unwrap_or_else(|| "-".into());
            let args = match &n.args {
                Args::Unknown => "-".to_string(),
                Args::Slots(s) => s.to_string(),
                Args::Types(v) if v.is_empty() => "0".to_string(),
                Args::Types(v) => v.iter().map(|x| x.0.clone()).collect::<Vec<_>>().join(", "),
            };
            let ret = match &n.ret {
                Ret::Unknown => "-".to_string(),
                Ret::Type(s, _) => s.clone(),
            };
            let params = if n.params.is_empty() { "-".into() } else { n.params.join(", ") };
            let doc = if n.doc.is_empty() { "-".into() } else { n.doc.replace(['\t', '\n'], " ") };
            t.push_str(&format!("{id}\t{args}\t{ret}\t{}\t{params}\t{doc}\n", n.name));
        }
        t
    }

    pub fn summary(&self) -> String {
        let known = self.natives.iter().filter(|n| n.id.is_some()).count();
        let typed = self.natives.iter().filter(|n| matches!(n.args, Args::Types(_))).count();
        let ns: std::collections::BTreeSet<&str> = self.natives.iter().map(|n| n.namespace()).collect();
        format!(
            "{} natives in {} namespaces; {} with an identifier, {} with typed arguments; directives: {}",
            self.natives.len(),
            ns.len(),
            known,
            typed,
            if self.directives.is_empty() { "none".to_string() } else { self.directives.iter().map(|d| format!("@{}", d.name)).collect::<Vec<_>>().join(", ") }
        )
    }
}

pub fn valid_name(n: &str) -> bool {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    match n.split_once("::") {
        Some((a, b)) => ok(a) && ok(b),
        None => ok(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "# test\nidentifier\targs\treturns\tname\tparams\tconfidence\n\
@native_table\t{ kind = \"buckets\", module = \"game.exe\", pattern = \"48 8D 0D ?? ?? ?? ??\", rip = { disp = 3, len = 7 } }\n\
@script_tick\t{ module = \"game.exe\", export = \"tick\" }\n\
0x1111111111111111\t0\tPed\tPLAYER::PLAYER_PED_ID\t-\tconfirmed\n\
0x2222\tPlayer, BOOL\tvoid\tPLAYER::SET_PLAYER_INVINCIBLE\tplayer, toggle\t-\n\
-\tEntity, BOOL\tVector3\tENTITY::GET_ENTITY_COORDS\t-\t-\n\
-\t7\t0\tENTITY::SET_ENTITY_COORDS_NO_OFFSET\t-\t-\n\
-\tHash, int*, int\tBOOL\tSTATS::STAT_GET_INT\t-\t-\n";

    #[test]
    fn reads_checks_and_writes() {
        let mut d = parse(T);
        d.check();
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        assert_eq!(d.natives.len(), 5);
        assert_eq!(d.directives.len(), 2);
        assert_eq!(d.directives[0].value.path("rip.len").and_then(|v| v.as_i64()), Some(7));
        let n = d.find("SET_PLAYER_INVINCIBLE").unwrap();
        assert_eq!(n.id, Some(Id::Hash(0x2222)));
        assert_eq!(n.params, vec!["player", "toggle"]);
        assert_eq!(n.args, Args::Types(vec![("Player".into(), Type::Int), ("BOOL".into(), Type::Bool)]));
        assert_eq!(d.find("ENTITY::SET_ENTITY_COORDS_NO_OFFSET").unwrap().args, Args::Slots(7));
        assert!(matches!(d.find("STAT_GET_INT").unwrap().args, Args::Types(ref v) if v[1].1 == Type::OutInt));
        // round trip
        let mut d2 = parse(&d.to_tsv());
        d2.check();
        assert!(d2.errors.is_empty(), "{:?}", d2.errors);
        assert_eq!(d2.natives.len(), 5);
        assert_eq!(d2.find("PLAYER_PED_ID").unwrap().id, Some(Id::Hash(0x1111111111111111)));
        // errors
        let mut bad = parse("identifier\targs\treturns\tname\n0xZZ\t0\tint\tA\n-\tfoo-bar\tint\tB\n-\t0\tint\tC\n-\t0\tint\tC\n@native_table\t{ kind = \"tree\" }\n");
        bad.check();
        assert_eq!(bad.errors.len(), 4, "{:?}", bad.errors);
    }
}
