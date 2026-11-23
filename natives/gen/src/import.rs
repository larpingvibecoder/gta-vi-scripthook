//! Importing natives from the public documentation's JSON form and merging identifiers.
//!
//! The documentation form is `{ "NS": { "0xHASH": { "name": "...", "params": [{ "type": "Ped",
//! "name": "ped" }], "return_type": "void", "comment": "..." } } }`. Its hashes belong to the
//! build the documentation describes, not necessarily to the game you own: by default only names,
//! types and parameter names are imported and identifiers stay unknown (`-`); `keep_ids` keeps
//! them (mark such a database as unverified).

use crate::db::{Args, Db, Id, Native, Ret};
use scripthook_sdk::config::{json, Value};
use scripthook_sdk::natives::Type;

pub fn from_doc_json(text: &str, keep_ids: bool) -> Result<Db, String> {
    let v = json::parse(text)?;
    let mut db = Db { columns: ["identifier", "args", "returns", "name", "params", "doc"].iter().map(|s| s.to_string()).collect(), ..Default::default() };
    let ns_list = v.as_table().ok_or("expected an object of namespaces")?;
    for (ns, entries) in ns_list {
        let Some(entries) = entries.as_table() else { continue };
        for (key, n) in entries {
            let Some(name) = n.str_of("name").filter(|s| !s.is_empty()) else {
                db.warnings.push(format!("{ns} {key}: no name (skipped)"));
                continue;
            };
            let full = if name.contains("::") { name.clone() } else { format!("{ns}::{name}") };
            let mut args = Vec::new();
            let mut params = Vec::new();
            let mut bad = None;
            if let Some(ps) = n.get("params").and_then(Value::as_array) {
                for p in ps {
                    let ty = p.str_of("type").unwrap_or_default();
                    match Type::parse(&ty) {
                        Some(t) if t != Type::Void => args.push((ty.clone(), t)),
                        _ => bad = Some(ty.clone()),
                    }
                    params.push(p.str_of("name").unwrap_or_default());
                }
            }
            if let Some(b) = &bad {
                db.warnings.push(format!("{full}: argument type '{b}' not understood (arguments left unknown)"));
            }
            let ret = n.str_of("return_type").or_else(|| n.str_of("results")).map(|r| match Type::parse(&r) {
                Some(t) if !t.is_out() => Ret::Type(r, t),
                _ => Ret::Unknown,
            });
            let id = if keep_ids { key.strip_prefix('_').unwrap_or(key).parse_id() } else { None };
            db.natives.push(Native {
                name: full,
                id,
                args: if bad.is_some() { Args::Unknown } else { Args::Types(args) },
                ret: ret.unwrap_or(Ret::Unknown),
                params,
                doc: n.str_of("comment").unwrap_or_default().lines().next().unwrap_or("").chars().take(160).collect(),
                cols: Vec::new(),
                line: 0,
            });
        }
    }
    Ok(db)
}

trait ParseId {
    fn parse_id(&self) -> Option<Id>;
}
impl ParseId for str {
    fn parse_id(&self) -> Option<Id> {
        Id::parse(self).ok()
    }
}

/// Applies identifiers from `ids` (a TSV with `name` and `identifier` columns, or two columns
/// `name<TAB>identifier` without header). Returns (updated, names not found).
pub fn merge_ids(db: &mut Db, ids: &str) -> Result<(usize, Vec<String>), String> {
    let mut updated = 0;
    let mut missing = Vec::new();
    let mut cols: Option<(usize, usize)> = None;
    for (ln, l) in ids.lines().enumerate() {
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() || l.starts_with('#') || l.starts_with('@') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').map(str::trim).collect();
        if cols.is_none() {
            let lower: Vec<String> = f.iter().map(|x| x.to_ascii_lowercase()).collect();
            if let (Some(n), Some(i)) = (lower.iter().position(|c| c == "name"), lower.iter().position(|c| c == "identifier" || c == "hash" || c == "index")) {
                cols = Some((n, i));
                continue;
            }
            cols = Some((0, 1));
        }
        let (ni, ii) = cols.unwrap();
        let (Some(name), Some(id)) = (f.get(ni), f.get(ii)) else { continue };
        if id.is_empty() || *id == "-" {
            continue;
        }
        let id = Id::parse(id).map_err(|e| format!("line {}: {e}", ln + 1))?;
        let u = name.to_ascii_uppercase();
        match db.natives.iter_mut().find(|n| n.name.to_ascii_uppercase() == u || n.short().to_ascii_uppercase() == u) {
            Some(n) => {
                n.id = Some(id);
                updated += 1;
            }
            None => missing.push(name.to_string()),
        }
    }
    Ok((updated, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_and_merges() {
        let j = r#"{"PLAYER": {"0x00000000000000A1": {"name": "PLAYER_PED_ID", "params": [], "return_type": "Ped", "comment": "the local player's ped"},
                     "0x00000000000000A2": {"name": "SET_PLAYER_INVINCIBLE", "params": [{"type": "Player", "name": "player"}, {"type": "BOOL", "name": "toggle"}], "return_type": "void"}}}"#;
        let mut d = from_doc_json(j, false).unwrap();
        assert_eq!(d.natives.len(), 2);
        assert!(d.natives.iter().all(|n| n.id.is_none()));
        let n = d.find("SET_PLAYER_INVINCIBLE").unwrap();
        assert_eq!(n.params, vec!["player", "toggle"]);
        assert_eq!(d.find("PLAYER_PED_ID").unwrap().doc, "the local player's ped");
        let (u, miss) = merge_ids(&mut d, "name\tidentifier\nPLAYER::PLAYER_PED_ID\t0x10\nNOPE\t0x11\n").unwrap();
        assert_eq!((u, miss), (1, vec!["NOPE".to_string()]));
        assert_eq!(d.find("PLAYER_PED_ID").unwrap().id, Some(Id::Hash(0x10)));
        let k = from_doc_json(j, true).unwrap();
        assert_eq!(k.find("PLAYER_PED_ID").unwrap().id, Some(Id::Hash(0x00000000000000A1)));
        let mut d2 = crate::db::parse(&d.to_tsv());
        d2.check();
        assert!(d2.errors.is_empty(), "{:?}", d2.errors);
    }
}
