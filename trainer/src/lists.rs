//! Trainer data lists (vehicles, weapons, peds, weathers, locations, ...): read from the owner's
//! trainer data folders. The repository ships no list data.
//!
//! Text lists (`.txt` / `.tsv` / `.csv`): tab-separated (comma / semicolon when a line has no
//! tab), `#` comments; a header row naming columns (`name x y z heading ...`), or `name 0xHASH
//! ...` rows, or hash-first rows `0xHASH name|- ...` (a column starting with `Player_` labels
//! the row). JSON / TOML lists: an array of names or objects (`name` / `model`, `label`, `hash`,
//! `x y z` or `pos` / `coords`, `heading`), the array under the list's name, or a map of
//! name → properties.

use crate::script::Item;
use scripthook_sdk::config::{json, toml, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Files in a data folder that are not lists.
pub fn is_list_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let ext_ok = [".txt", ".tsv", ".csv", ".json", ".toml"].iter().any(|e| l.ends_with(e));
    let stem = l.rsplit_once('.').map(|x| x.0).unwrap_or(&l);
    ext_ok && !["natives", "features", "trainer", "readme", "cheats", "state", "menu", "mod"].contains(&stem)
}

/// The list name of a file: its stem, lower-case (`weather` → `weathers`).
pub fn list_name(file: &str) -> String {
    let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(file).to_ascii_lowercase();
    match stem.as_str() {
        "weather" => "weathers".into(),
        "vehicle" => "vehicles".into(),
        "weapon" => "weapons".into(),
        "ped" | "skins" | "models" => "peds".into(),
        "location" | "teleports" => "locations".into(),
        _ => stem,
    }
}

fn parse_hash(s: &str) -> Option<u32> {
    let t = s.trim();
    t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).and_then(|h| u32::from_str_radix(h, 16).ok())
}

/// "LOCATION_STARLETMOTEL_NAME" → "Starletmotel"
fn pretty_key(s: &str) -> String {
    match s.strip_prefix("LOCATION_").and_then(|x| x.strip_suffix("_NAME")) {
        Some(k) => {
            let words: Vec<String> = k.split('_').filter(|w| !w.is_empty()).map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase(),
                    None => String::new(),
                }
            }).collect();
            words.join(" ")
        }
        None => s.to_string(),
    }
}

pub fn parse_text(text: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut header: Option<Vec<String>> = None;
    let mut first = true;
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        let f: Vec<&str> = if l.contains('\t') { l.split('\t') } else if l.contains(',') { l.split(',') } else { l.split(';') }.map(str::trim).collect();
        if first {
            first = false;
            let lower: Vec<String> = f.iter().map(|x| x.to_ascii_lowercase()).collect();
            if lower.iter().any(|c| c == "name" || c == "model") && parse_hash(f[0]).is_none() {
                header = Some(lower);
                continue;
            }
        }
        if let Some(h) = &header {
            let get = |n: &str| h.iter().position(|c| c == n).and_then(|i| f.get(i)).copied().filter(|x| !x.is_empty() && *x != "-");
            let Some(name) = get("name").or(get("model")) else { continue };
            let num = |n: &str| get(n).and_then(|x| x.parse::<f64>().ok());
            let pos = match (num("x"), num("y"), num("z")) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            };
            items.push(Item { name: name.to_string(), label: get("label").map(str::to_string).unwrap_or_else(|| pretty_key(name)), hash: get("hash").and_then(parse_hash), pos, heading: num("heading") });
            continue;
        }
        if let Some(h) = parse_hash(f[0]) {
            // hash-first: 0xHASH name|- ... (a Player_ tag labels it)
            let name = f.get(1).copied().filter(|x| !x.is_empty() && *x != "-");
            let tag = f.iter().skip(2).find(|x| x.starts_with("Player_")).copied();
            let n = name.map(str::to_string).unwrap_or_else(|| format!("0x{h:08X}"));
            let label = match tag {
                Some(t) => format!("{t} ({n})"),
                None => n.clone(),
            };
            items.push(Item { name: n, label, hash: Some(h), pos: None, heading: None });
        } else {
            let hash = f.get(1).and_then(|x| parse_hash(x));
            items.push(Item { name: f[0].to_string(), label: pretty_key(f[0]), hash, pos: None, heading: None });
        }
    }
    items
}

fn item_of(name_key: Option<&str>, v: &Value) -> Option<Item> {
    match v {
        Value::Str(s) => Some(Item { name: s.clone(), label: s.clone(), ..Default::default() }),
        Value::Table(_) => {
            let name = v.str_of("name").or_else(|| v.str_of("model")).or_else(|| name_key.map(str::to_string))?;
            let pos = v
                .get("pos")
                .or_else(|| v.get("coords"))
                .and_then(|p| match p {
                    Value::Array(a) if a.len() >= 3 => Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]),
                    Value::Table(_) => Some([p.f64_of("x")?, p.f64_of("y")?, p.f64_of("z")?]),
                    _ => None,
                })
                .or_else(|| Some([v.f64_of("x")?, v.f64_of("y")?, v.f64_of("z")?]));
            let hash = match v.get("hash") {
                Some(Value::Int(i)) => Some(*i as u32),
                Some(Value::Str(s)) => parse_hash(s),
                _ => None,
            };
            Some(Item { label: v.str_of("label").unwrap_or_else(|| pretty_key(&name)), name, hash, pos, heading: v.f64_of("heading") })
        }
        _ => None,
    }
}

pub fn parse_structured(v: &Value, list: &str) -> Vec<Item> {
    let v = v.get(list).unwrap_or(v);
    match v {
        Value::Array(a) => a.iter().filter_map(|x| item_of(None, x)).collect(),
        Value::Table(t) => t.iter().filter_map(|(k, x)| item_of(Some(k), x)).collect(),
        _ => Vec::new(),
    }
}

/// Every list in the folders (later folders add to / replace earlier lists of the same name).
pub fn load(dirs: &[std::path::PathBuf]) -> (BTreeMap<String, Vec<Item>>, Vec<String>) {
    let mut lists: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    let mut errors = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files {
            let fname = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !is_list_file(&fname) {
                continue;
            }
            let name = list_name(&fname);
            match read_list(&p, &name) {
                Ok(items) if !items.is_empty() => {
                    lists.insert(name, items);
                }
                Ok(_) => {}
                Err(e) => errors.push(format!("{}: {e}", p.display())),
            }
        }
    }
    (lists, errors)
}

pub fn read_list(p: &Path, name: &str) -> Result<Vec<Item>, String> {
    let t = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
    let l = p.to_string_lossy().to_ascii_lowercase();
    if l.ends_with(".json") {
        Ok(parse_structured(&json::parse(&t)?, name))
    } else if l.ends_with(".toml") {
        Ok(parse_structured(&toml::parse(&t)?, name))
    } else {
        Ok(parse_text(&t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_forms() {
        let v = parse_text("# model\thash\nadder\t0xB779A091\ttest\nbus\t0x00000002\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].hash, Some(0xB779A091));
        let l = parse_text("name\tx\ty\tz\theading\tsource\nPier\t10\t20\t30\t90\tt\nLOCATION_HILLTOP_NAME\t-5\t6\t70\t\tt\n");
        assert_eq!(l[0].pos, Some([10.0, 20.0, 30.0]));
        assert_eq!(l[0].heading, Some(90.0));
        assert_eq!(l[1].label, "Hilltop");
        assert_eq!(l[1].heading, None);
        let p = parse_text("# joaat\tname\n0xE472B687\t-\t-\tHuman\tGEN\t-\t-\t-\tPlayer_A\t-\n0x00000010\tsome_ped\n");
        assert_eq!(p[0].label, "Player_A (0xE472B687)");
        assert_eq!(p[1].name, "some_ped");
        let w = parse_text("CLEAR\t0x1\nRAIN\t0x2\n");
        assert_eq!(w[1].name, "RAIN");
        assert_eq!(parse_text("a,1\nb,2").len(), 2);
        assert!(is_list_file("vehicles.txt") && !is_list_file("natives.tsv") && !is_list_file("features.tsv"));
        assert_eq!(list_name("weather.txt"), "weathers");
    }

    #[test]
    fn structured_forms() {
        let j = json::parse(r#"{"locations": [{"name": "Pier", "pos": [1, 2, 3], "heading": 90}, {"name": "Hill", "x": 4, "y": 5, "z": 6}]}"#).unwrap();
        let v = parse_structured(&j, "locations");
        assert_eq!(v[0].pos, Some([1.0, 2.0, 3.0]));
        assert_eq!(v[1].pos, Some([4.0, 5.0, 6.0]));
        let t = toml::parse("[adder]\nhash = \"0x10\"\n[bus]\nlabel = \"Bus\"\n").unwrap();
        let v = parse_structured(&t, "vehicles");
        assert_eq!(v[0].name, "adder");
        assert_eq!(v[0].hash, Some(0x10));
        assert_eq!(v[1].label, "Bus");
        let a = json::parse(r#"["CLEAR", "RAIN"]"#).unwrap();
        assert_eq!(parse_structured(&a, "weathers").len(), 2);
    }
}
