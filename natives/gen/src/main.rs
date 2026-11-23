//! `shk-natives`: the natives database tool (docs/natives-database.md).

use shk_natives::{db, gen, import, pe, verify};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "shk-natives: the gta-vi-scripthook natives database tool

usage:
  shk-natives check <natives.tsv>                       parse and check; prints a summary
  shk-natives gen <natives.tsv> --out <dir> [--lang cpp,c,rust]
                                                        writes natives.h, natives_c.h, natives.rs
  shk-natives fmt <natives.tsv> [--out <file>]          canonical form (sorted rows)
  shk-natives import-json <doc.json> --out <natives.tsv> [--keep-identifiers]
                                                        names, types and parameters from the public
                                                        documentation's JSON form (your own copy)
  shk-natives merge <natives.tsv> <ids.tsv> --out <natives.tsv>
                                                        identifiers you found (name, identifier)
  shk-natives verify <natives.tsv> --exe <game.exe>     checks @native_table / @script_tick
                                                        locations against your executable (read-only)
  shk-natives coverage <natives.tsv> <names.txt>        which of the listed natives (one per line)
                                                        have an identifier
  shk-natives pe-info --exe <file.exe>                  sections of an executable

Single-player / offline tool: it reads files you give it and writes only where you say.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("shk-natives: {e}");
            ExitCode::from(1)
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn read(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))
}

fn load(p: &str) -> Result<db::Db, String> {
    let mut d = db::parse(&read(p)?);
    d.check();
    for w in &d.warnings {
        eprintln!("warning: {w}");
    }
    if !d.errors.is_empty() {
        for e in &d.errors {
            eprintln!("error: {e}");
        }
        return Err(format!("{p}: {} error(s)", d.errors.len()));
    }
    Ok(d)
}

fn write(p: &Path, text: &str) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    println!("wrote {}", p.display());
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let pos = |i: usize| args.get(i).filter(|a| !a.starts_with("--")).cloned().ok_or_else(|| format!("missing argument\n\n{USAGE}"));
    match cmd {
        "check" => {
            let d = load(&pos(1)?)?;
            println!("{}", d.summary());
            Ok(())
        }
        "gen" => {
            let src = pos(1)?;
            let d = load(&src)?;
            let out = PathBuf::from(flag(args, "--out").ok_or("--out <dir> is required")?);
            let langs = flag(args, "--lang").unwrap_or_else(|| "cpp,c,rust".into());
            let name = Path::new(&src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(src.clone());
            for l in langs.split(',') {
                match l.trim() {
                    "cpp" | "c++" => write(&out.join("natives.h"), &gen::cpp(&d, &name))?,
                    "c" => write(&out.join("natives_c.h"), &gen::c(&d, &name))?,
                    "rust" | "rs" => write(&out.join("natives.rs"), &gen::rust(&d, &name))?,
                    o => return Err(format!("unknown language {o}")),
                }
            }
            Ok(())
        }
        "fmt" => {
            let src = pos(1)?;
            let d = load(&src)?;
            match flag(args, "--out") {
                Some(o) => write(Path::new(&o), &d.to_tsv()),
                None => {
                    print!("{}", d.to_tsv());
                    Ok(())
                }
            }
        }
        "import-json" => {
            let d = import::from_doc_json(&read(&pos(1)?)?, args.iter().any(|a| a == "--keep-identifiers"))?;
            for w in &d.warnings {
                eprintln!("warning: {w}");
            }
            let out = flag(args, "--out").ok_or("--out <natives.tsv> is required")?;
            println!("{}", d.summary());
            write(Path::new(&out), &d.to_tsv())
        }
        "merge" => {
            let mut d = load(&pos(1)?)?;
            let (n, missing) = import::merge_ids(&mut d, &read(&pos(2)?)?)?;
            for m in &missing {
                eprintln!("warning: {m}: not in the database");
            }
            println!("{n} identifier(s) applied, {} not found", missing.len());
            d.check();
            if !d.errors.is_empty() {
                return Err(d.errors.join("\n"));
            }
            write(Path::new(&flag(args, "--out").ok_or("--out <natives.tsv> is required")?), &d.to_tsv())
        }
        "verify" => {
            let d = load(&pos(1)?)?;
            let exe = flag(args, "--exe").ok_or("--exe <game.exe> is required")?;
            let p = pe::Pe::parse(std::fs::read(&exe).map_err(|e| format!("{exe}: {e}"))?)?;
            let name = Path::new(&exe).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let checks = verify::verify(&d, &p, &name);
            if checks.is_empty() {
                println!("no @native_table / @script_tick location to check");
            }
            let mut bad = 0;
            for c in checks {
                let t = match c.outcome {
                    verify::Outcome::Found(r) => format!("found at RVA {r:#x} (address {:#x})", p.image_base + r as u64),
                    verify::Outcome::Ambiguous(n, r) => {
                        bad += 1;
                        format!("AMBIGUOUS: the pattern matches {n} times; first result at RVA {r:#x}")
                    }
                    verify::Outcome::NotFound(e) => {
                        bad += 1;
                        format!("NOT FOUND: {e}")
                    }
                    verify::Outcome::Skipped(e) => format!("not checked: {e}"),
                };
                println!("@{}: {t}", c.directive);
            }
            if bad > 0 {
                Err(format!("{bad} location(s) do not match"))
            } else {
                Ok(())
            }
        }
        "coverage" => {
            let d = load(&pos(1)?)?;
            let names = read(&pos(2)?)?;
            let (mut known, mut missing) = (0, Vec::new());
            for n in names.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                match d.find(n) {
                    Some(x) if x.id.is_some() => known += 1,
                    Some(_) => missing.push(format!("{n} (listed, no identifier)")),
                    None => missing.push(format!("{n} (not in the database)")),
                }
            }
            for m in &missing {
                println!("missing: {m}");
            }
            println!("{known} of {} needed natives have an identifier", known + missing.len());
            Ok(())
        }
        "pe-info" => {
            let exe = flag(args, "--exe").ok_or("--exe <file> is required")?;
            let p = pe::Pe::parse(std::fs::read(&exe).map_err(|e| format!("{exe}: {e}"))?)?;
            println!("image base {:#x}, {} sections", p.image_base, p.sections.len());
            for s in &p.sections {
                println!("  {:<8} rva {:#010x} size {:#010x} file {:#010x}+{:#x}", s.name, s.va, s.vsize, s.raw_off, s.raw_size);
            }
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        o => Err(format!("unknown command {o}\n\n{USAGE}")),
    }
}
