//! Checks a database's `@native_table` / `@script_tick` locations against an executable file.

use crate::db::Db;
use crate::pe::Pe;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// found exactly where the location says: RVA of the result
    Found(u32),
    /// the pattern matches more than once (count); the first result's RVA
    Ambiguous(usize, u32),
    NotFound(String),
    /// cannot be checked in the file (another module, a pointer chain, no location)
    Skipped(String),
}

pub struct Check {
    pub directive: String,
    pub outcome: Outcome,
}

pub fn verify(db: &Db, pe: &Pe, exe_name: &str) -> Vec<Check> {
    let mut out = Vec::new();
    for d in db.directives.iter().filter(|d| d.name == "native_table" || d.name == "script_tick") {
        let v = &d.value;
        let outcome = (|| {
            if let Some(m) = v.str_of("module") {
                if !m.is_empty() && !m.eq_ignore_ascii_case(exe_name) && !exe_name.to_ascii_lowercase().starts_with(&format!("{}.", m.to_ascii_lowercase())) {
                    return Outcome::Skipped(format!("location is in module {m}, not {exe_name}"));
                }
            }
            let offset = v.i64_of("offset").unwrap_or(0);
            let base: Option<(u32, Option<usize>)>;
            if let Some(e) = v.str_of("export") {
                match pe.export(&e) {
                    Some(r) => base = Some((r, None)),
                    None => return Outcome::NotFound(format!("no export {e}")),
                }
            } else if let Some(p) = v.str_of("pattern") {
                let sec = v.str_of("section");
                let nth = v.i64_of("nth").unwrap_or(0).max(0) as usize;
                match pe.scan(&p, sec.as_deref(), 64) {
                    Err(e) => return Outcome::NotFound(e),
                    Ok(m) if m.is_empty() => return Outcome::NotFound(format!("pattern not found{}", sec.map(|s| format!(" in {s}")).unwrap_or_default())),
                    Ok(m) if nth >= m.len() => return Outcome::NotFound(format!("pattern found {} time(s), nth = {nth}", m.len())),
                    Ok(m) => base = Some((m[nth], Some(m.len()))),
                }
            } else if let Some(a) = v.i64_of("address") {
                let rva = (a as u64).wrapping_sub(pe.image_base);
                if rva < u32::MAX as u64 && pe.offset(rva as u32).is_some() {
                    base = Some((rva as u32, None));
                } else {
                    return Outcome::Skipped("an absolute address outside the image file".into());
                }
            } else {
                return Outcome::Skipped("no location".into());
            }
            let (mut rva, count) = base.unwrap();
            if let Some(rip) = v.get("rip") {
                let disp = rip.i64_of("disp").unwrap_or(3) as u32;
                let len = rip.i64_of("len").unwrap_or(7) as u32;
                match pe.read_u32(rva + disp) {
                    Some(d) => rva = (rva as i64 + len as i64 + d as i32 as i64) as u32,
                    None => return Outcome::NotFound("RIP displacement unreadable".into()),
                }
            }
            rva = (rva as i64 + offset) as u32;
            if v.get("chain").is_some() {
                return Outcome::Skipped(format!("base at RVA {rva:#x}; the pointer chain is followed at run time"));
            }
            match count {
                Some(n) if n > 1 => Outcome::Ambiguous(n, rva),
                _ => Outcome::Found(rva),
            }
        })();
        out.push(Check { directive: d.name.clone(), outcome });
    }
    out
}
