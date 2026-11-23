//! Test helpers: finding Sonoma Runtime's `srt.exe` and its synthetic test title (read-only; both
//! are copied into a scratch folder so a rebuild elsewhere cannot change them mid-test), building
//! the Rust plugins, laying out mod folders and trainer data, reading the title's summary line.

pub mod target;

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// MSVC-built C / C++ parts (empty when MSVC is missing).
pub fn c_build_dir() -> Option<PathBuf> {
    let d = env!("SHK_TEST_BUILD_DIR");
    (!d.is_empty()).then(|| PathBuf::from(d))
}

/// Sonoma Runtime's checkout: `SHK_SONOMA_RUNTIME` or `..\sonoma-runtime` next to this repository.
pub fn sonoma_runtime() -> Option<PathBuf> {
    let p = std::env::var("SHK_SONOMA_RUNTIME").map(PathBuf::from).unwrap_or_else(|_| repo().parent().unwrap().join("sonoma-runtime"));
    p.is_dir().then_some(p)
}

/// `srt.exe`: `SHK_SRT_EXE`, else `<sonoma-runtime>\target\release\srt.exe`.
pub fn srt_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SHK_SRT_EXE") {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    Some(sonoma_runtime()?.join("target").join("release").join("srt.exe")).filter(|p| p.is_file())
}

/// The folder with `srt_test_title.exe` (built by Sonoma Runtime's tests): `SHK_SRT_TITLE_DIR`,
/// else the newest under `<sonoma-runtime>\target\release\build`.
pub fn title_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SHK_SRT_TITLE_DIR") {
        return Some(PathBuf::from(p)).filter(|p| p.join("srt_test_title.exe").is_file());
    }
    let build = sonoma_runtime()?.join("target").join("release").join("build");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack = vec![(build, 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                stack.push((p, depth + 1));
            } else if p.file_name().map(|n| n == "srt_test_title.exe").unwrap_or(false) {
                let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                let dir = p.parent().unwrap().to_path_buf();
                if dir.join("title_dll.dll").is_file() && best.as_ref().map(|b| t > b.0).unwrap_or(true) {
                    best = Some((t, dir));
                }
            }
        }
    }
    best.map(|b| b.1)
}

pub fn scratch(name: &str) -> PathBuf {
    let d = repo().join("target").join("shk-test").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn write(p: &Path, t: impl AsRef<[u8]>) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, t).unwrap();
}

/// Builds the Rust plugins (release) and returns `target\release`.
pub fn build_rust_plugins() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let o = Command::new(cargo).current_dir(repo()).env("CARGO_BUILD_JOBS", std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "6".into())).args(["build", "--release", "-p", "shk-menu", "-p", "shk-trainer", "-p", "hello-rust"]).output().expect("cargo");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    repo().join("target").join("release")
}

const CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Game configVersion="1">
  <Identity Name="Sonoma.TestTitle" Publisher="CN=Sonoma Runtime Tests" Version="1.0.0.0" />
  <TitleId>5352544E</TitleId>
  <ExecutableList><Executable Name="srt_test_title.exe" Id="Game" /></ExecutableList>
  <ShellVisuals DefaultDisplayName="Sonoma Runtime test title" PublisherDisplayName="Sonoma" />
</Game>
"#;

pub struct Rig {
    pub base: PathBuf,
    pub srt: PathBuf,
    pub pkg: PathBuf,
    pub data: PathBuf,
    pub mods: PathBuf,
    pub trainer_data: PathBuf,
}

/// A scratch copy of srt.exe and the test title package.
pub fn rig(name: &str) -> Option<Rig> {
    let (srt, title) = (srt_exe()?, title_dir()?);
    let base = scratch(name);
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(&srt, bin.join("srt.exe")).unwrap();
    let pkg = base.join("package");
    std::fs::create_dir_all(pkg.join("data")).unwrap();
    for f in ["srt_test_title.exe", "title_dll.dll", "title_missing.dll"] {
        std::fs::copy(title.join(f), pkg.join(f)).unwrap();
    }
    write(&pkg.join("MicrosoftGame.config"), CONFIG);
    write(&pkg.join("data").join("mod_target.txt"), "original text");
    write(&pkg.join("data").join("patched.bin"), "original-original-original-orig!");
    let data = base.join("data");
    let mods = data.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    Some(Rig { srt: bin.join("srt.exe"), pkg, mods, trainer_data: base.join("trainer-data"), data, base })
}

impl Rig {
    /// A mod folder with a plugin DLL and extra files.
    pub fn add_mod(&self, id: &str, dll: &Path, manifest: &Path, extra: &[(&str, &Path)]) {
        let d = self.mods.join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(dll, d.join(dll.file_name().unwrap())).unwrap();
        std::fs::copy(manifest, d.join("mod.toml")).unwrap();
        for (name, src) in extra {
            let to = d.join(name);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(src, to).unwrap();
        }
    }

    /// Trainer data for the test title: its natives (with or without the script tick) and small
    /// synthetic lists (no game data).
    pub fn trainer_data(&self, tick: bool) {
        let t = std::fs::read_to_string(repo().join("natives").join("examples").join("test-title.tsv")).unwrap();
        let t: String = if tick { t } else { t.lines().filter(|l| !l.starts_with("@script_tick")).map(|l| format!("{l}\n")).collect() };
        let d = &self.trainer_data;
        write(&d.join("natives.tsv"), t);
        let j = scripthook_sdk::joaat;
        write(&d.join("vehicles.txt"), format!("# model\thash\nadder\t{:#010X}\ttest\nbus\t{:#010X}\ttest\n", j("adder"), j("bus")));
        write(&d.join("weather.txt"), "# name\thash\nCLEAR\t0x00000001\nRAIN\t0x00000002\n");
        write(&d.join("locations.tsv"), "name\tx\ty\tz\theading\tsource\nPier\t10\t20\t30\t90\ttest\nLOCATION_HILLTOP_NAME\t-5\t6\t70\t\ttest\n");
        write(&d.join("weapons.txt"), "# name\thash\nWEAPON_TEST\t0x00001234\n");
        write(&d.join("peds.txt"), "# hash\tname\n0x00000042\t-\t-\tHuman\t-\t-\t-\t-\tPlayer_Test\t-\n");
        // a memory cheat with a known location on the test title: its armour field
        write(
            &d.join("cheats.toml"),
            "[[location]]\nid = \"test_armour\"\ntype = \"f32\"\nmodule = \"srt_test_title.exe\"\nexport = \"g_player\"\noffset = 24\n\n\
             [[cheat]]\nid = \"test_armour\"\nname = \"Armour 77 (test title)\"\ngroup = \"Player\"\nkind = \"freeze\"\ntype = \"f32\"\nvalue = 77\nlocation = \"test_armour\"\n",
        );
    }

    /// `srt run <package> --data <data> -- --mods-test <frames>`: (exit code, log, report.json text).
    pub fn run(&self, frames: u32, env: &[(&str, String)]) -> (i32, String, String) {
        let mut c = Command::new(&self.srt);
        c.arg("run").arg(&self.pkg).arg("--data").arg(&self.data).args(["--quiet", "--", "--mods-test"]).arg(frames.to_string());
        for (k, v) in env {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        let log = std::fs::read_to_string(self.data.join("logs").join("srt.log")).unwrap_or_default();
        let report = std::fs::read_to_string(self.data.join("report.json")).unwrap_or_default();
        (o.status.code().unwrap_or(-1), log, report)
    }
}

/// `key=value` fields of the test title's summary line ("mods test: money=...").
pub fn summary(log: &str) -> std::collections::HashMap<String, String> {
    let line = log.lines().find(|l| l.contains("mods test: money=")).unwrap_or_else(|| panic!("no summary in\n{log}"));
    let rest = &line[line.find("money=").unwrap()..];
    let mut m = std::collections::HashMap::new();
    let (mut key, mut cur, mut depth, mut in_key) = (String::new(), String::new(), 0, true);
    for ch in rest.chars() {
        match ch {
            '=' if in_key => in_key = false,
            '(' => {
                depth += 1;
                cur.push(ch)
            }
            ')' => {
                depth -= 1;
                cur.push(ch)
            }
            ' ' if depth == 0 && !in_key => {
                m.insert(std::mem::take(&mut key), std::mem::take(&mut cur));
                in_key = true;
            }
            _ if in_key => key.push(ch),
            _ => cur.push(ch),
        }
    }
    if !key.is_empty() {
        m.insert(key, cur);
    }
    m
}

pub fn num(m: &std::collections::HashMap<String, String>, k: &str) -> f64 {
    m.get(k).unwrap_or_else(|| panic!("{k} missing: {m:?}")).trim().parse().unwrap_or_else(|_| panic!("{k}: {:?}", m[k]))
}

pub fn vec3(m: &std::collections::HashMap<String, String>, k: &str) -> [f64; 3] {
    let v: Vec<f64> = m[k].trim_matches(['(', ')']).split(',').map(|x| x.parse().unwrap()).collect();
    [v[0], v[1], v[2]]
}
pub use shk_natives;
