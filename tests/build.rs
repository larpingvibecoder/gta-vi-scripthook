//! Builds the C and C++ parts the tests need with MSVC: the hello-c / hello-cpp examples (with
//! the version-2 adapter), the ABI check program, and a compile check of the generated
//! `natives.h` / `natives_c.h`. When no MSVC is found the tests that need them are skipped
//! (`SHK_TEST_BUILD_DIR` is empty). Dev/test-only.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn msvc_env() -> Option<HashMap<String, String>> {
    let pf = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let vswhere = PathBuf::from(pf).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let out = Command::new(&vswhere).args(["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]).output().ok()?;
    let dir = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    let vcvars = Path::new(&dir).join(r"VC\Auxiliary\Build\vcvars64.bat");
    if !vcvars.is_file() {
        return None;
    }
    let out = Command::new("cmd").raw_arg(format!("/d /s /c \"call \"{}\" >nul 2>&1 && set\"", vcvars.display())).output().ok()?;
    let env: HashMap<String, String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect();
    env.contains_key("INCLUDE").then_some(env)
}

fn cl(env: &HashMap<String, String>, dir: &Path) -> Command {
    let path = env.get("PATH").or_else(|| env.get("Path")).cloned().unwrap_or_default();
    let exe = std::env::split_paths(&path).map(|d| d.join("cl.exe")).find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("cl.exe"));
    let mut c = Command::new(exe);
    c.env_clear().envs(env).current_dir(dir);
    c
}

fn run(mut c: Command, what: &str) -> bool {
    match c.output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let t = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            println!("cargo:warning={what} failed: {}", t.lines().filter(|l| l.contains("error") || l.contains("warning")).collect::<Vec<_>>().join(" | "));
            false
        }
        Err(e) => {
            println!("cargo:warning={what}: {e}");
            false
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    for p in ["sdk/include", "sdk/c", "examples/hello-c", "examples/hello-cpp", "tests/c", "natives/examples"] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("c");
    let _ = std::fs::create_dir_all(&out);
    // generated headers from the example database (compile check below)
    let tsv = std::fs::read_to_string(root.join("natives/examples/test-title.tsv")).expect("natives/examples/test-title.tsv");
    let mut db = shk_natives::db::parse(&tsv);
    db.check();
    assert!(db.errors.is_empty(), "{:?}", db.errors);
    std::fs::write(out.join("natives.h"), shk_natives::gen::cpp(&db, "test-title.tsv")).unwrap();
    std::fs::write(out.join("natives_c.h"), shk_natives::gen::c(&db, "test-title.tsv")).unwrap();
    std::fs::write(out.join("natives.rs"), shk_natives::gen::rust(&db, "test-title.tsv")).unwrap();
    let Some(env) = msvc_env() else {
        println!("cargo:rustc-env=SHK_TEST_BUILD_DIR=");
        return;
    };
    let inc = format!("/I{}", root.join("sdk/include").display());
    let adapter = root.join("sdk/c/scripthook_srt2.c");
    let common = ["/nologo", "/W4", "/O2", "/MT", "/D_CRT_SECURE_NO_WARNINGS"];
    let mut ok = true;
    let mut c = cl(&env, &out);
    c.args(common).arg(&inc).arg("/LD").arg("/TC").arg(root.join("examples/hello-c/hello.c")).arg(&adapter).args(["/Fe:hello_c.dll", "/link", "/NOLOGO"]);
    ok &= run(c, "cl hello-c");
    let mut c = cl(&env, &out);
    c.args(common).arg(&inc).args(["/LD", "/std:c++17", "/EHsc"]).arg(root.join("examples/hello-cpp/hello.cpp")).arg("/Tc").arg(&adapter).args(["/Fe:hello_cpp.dll", "/link", "/NOLOGO"]);
    ok &= run(c, "cl hello-cpp");
    let mut c = cl(&env, &out);
    c.args(common).arg(&inc).arg("/TC").arg(root.join("tests/c/abi_check.c")).args(["/Fe:abi_check.exe", "/link", "/NOLOGO"]);
    ok &= run(c, "cl abi_check");
    // the generated headers compile (C++ and C)
    let mut c = cl(&env, &out);
    c.args(common).arg(&inc).arg(format!("/I{}", out.display())).args(["/c", "/std:c++17", "/EHsc"]).arg(root.join("tests/c/natives_compile.cpp"));
    ok &= run(c, "cl natives.h");
    let mut c = cl(&env, &out);
    c.args(common).arg(&inc).arg(format!("/I{}", out.display())).args(["/c", "/TC"]).arg(root.join("tests/c/natives_compile.c"));
    ok &= run(c, "cl natives_c.h");
    println!("cargo:rustc-env=SHK_TEST_BUILD_DIR={}", if ok { out.display().to_string() } else { String::new() });
}
