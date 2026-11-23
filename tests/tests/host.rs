//! The plugins loaded by the current Sonoma Runtime (`srt.exe`, plugin API version 2 through the
//! SDK's adapters) on its synthetic test title: the three hello examples, the menu (mirror mode:
//! this host cannot draw for plugins yet) and the trainer (natives through the test title's
//! synthetic handler table, toggles on its script thread, a memory cheat, list picks, typed names,
//! fly, the host menu driving a mirrored slider). Skipped when srt.exe or the test title is
//! missing. Screenshots of the host menu go to target\shk-shots.

use shk_tests::*;
use std::path::PathBuf;

fn shots_dir() -> PathBuf {
    let d = repo().join("target").join("shk-shots");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn line<'a>(log: &'a str, needle: &str) -> &'a str {
    log.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {needle:?} in\n{log}"))
}

#[test]
fn examples_menu_and_trainer_on_the_test_title() {
    let Some(rig) = rig("full") else {
        eprintln!("srt.exe or Sonoma Runtime's test title not found: skipping");
        return;
    };
    let rel = build_rust_plugins();
    let r = repo();
    rig.add_mod("scripthook-menu", &rel.join("shk_menu.dll"), &r.join("menu/mod.toml"), &[("menu.toml", &r.join("menu/menu.toml"))]);
    rig.add_mod("scripthook-trainer", &rel.join("shk_trainer.dll"), &r.join("trainer/mod.toml"), &[("trainer.toml", &r.join("trainer/trainer.toml"))]);
    rig.add_mod("hello-rust", &rel.join("hello_rust.dll"), &r.join("examples/hello-rust/mod.toml"), &[]);
    let c = c_build_dir();
    if let Some(c) = &c {
        rig.add_mod("hello-c", &c.join("hello_c.dll"), &r.join("examples/hello-c/mod.toml"), &[]);
        rig.add_mod("hello-cpp", &c.join("hello_cpp.dll"), &r.join("examples/hello-cpp/mod.toml"), &[]);
    }
    rig.trainer_data(true);
    // saved state: three toggles on at start
    write(&rig.mods.join("scripthook-trainer").join("state.toml"), "[on]\nhealth = true\nshow_fps = true\nshow_coords = true\n");
    let auto = rig.base.join("trainer-script.txt");
    write(
        &auto,
        "wait 10\nfeature health expect on\nfeature wanted_level value 3\nfeature add_money value 5000\nfeature add_money run\nfeature weather pick RAIN\nfeature set_hour value 18\n\
         feature tp_location pick Pier\nwait 20\nfeature spawn_vehicle text adder\nwait 30\ncheat test_armour on\nfeature heal run\nwait 40\nfeature fly on\nwait 60\nlog end\n",
    );
    let shots = shots_dir();
    let host_script = rig.base.join("menu-script.txt");
    write(
        &host_script,
        format!(
            "wait 60\nkey F7\nkey F8\nkey F9\nwait 30\nopen\npage plugin Trainer > World\nwait 2\nkey Right\nwait 3\nscreenshot {}\nwait 2\npage plugin Trainer > Player\nwait 3\nscreenshot {}\nwait 2\npage plugin gta-vi-scripthook\nwait 3\nscreenshot {}\nwait 2\nclose\nwait 5\n",
            shots.join("test-title-mirror-world.png").display(),
            shots.join("test-title-mirror-player.png").display(),
            shots.join("test-title-mirror-about.png").display()
        ),
    );
    let (code, log, report) = rig.run(
        330,
        &[
            ("SRT_INPUT", "synthetic:0,0,1".into()),
            ("SRT_TRAINER_DATA", rig.trainer_data.display().to_string()),
            ("SHK_TRAINER_SCRIPT", auto.display().to_string()),
            ("SRT_MENU_SCRIPT", host_script.display().to_string()),
        ],
    );
    std::fs::write(rig.base.join("srt.log"), &log).unwrap();
    assert_eq!(code, 0, "{log}");
    // the menu plugin: mirror mode, the service published, the trainer using it
    assert!(log.contains("menu: mirror mode"), "{log}");
    assert!(log.contains("menu: service scripthook.menu published"), "{log}");
    assert!(log.contains("trainer: pages in the scripthook menu service"), "{log}");
    assert!(log.contains("scripts: running on the game's script thread"), "{log}");
    // the examples: a native on the script thread, then their hotkeys
    let mut langs = vec!["rust"];
    if c.is_some() {
        langs.extend(["c", "cpp"]);
    }
    for (lang, key) in [("c", "F7"), ("cpp", "F8"), ("rust", "F9")] {
        if !langs.contains(&lang) {
            continue;
        }
        assert!(line(&log, &format!("hello-{lang}: native PLAYER_PED_ID ->")).contains("100 at ("), "hello-{lang}");
        assert!(line(&log, &format!("hello-{lang}: {key} -> ped 100 at (")).contains("ped 100"));
        assert!(log.contains(&format!("hello-{lang}: loaded on Sonoma Runtime")), "{log}");
        assert!(log.contains(&format!("hello-{lang}: unloaded")), "{log}");
    }
    // the trainer
    // one expected failure: "heal" needs natives the test title does not have
    assert!(log.contains("trainer script: done, 1 failure(s)"), "{log}");
    assert!(line(&log, "trainer script: feature heal run").contains("FAILED: needs native ENTITY::SET_ENTITY_HEALTH"), "{log}");
    let s = summary(&log);
    assert_eq!(num(&s, "invincible"), 1.0, "invincible on from the saved state");
    assert_eq!(num(&s, "health"), 100.0, "no damage while invincible");
    assert_eq!(num(&s, "wanted"), 3.0);
    assert_eq!(num(&s, "hour"), 19.0, "18 from the automation, +1 from the host menu's slider (mirrored item)");
    assert_eq!(s["weather"], "RAIN");
    assert_eq!(num(&s, "vehicles"), 1.0);
    assert_eq!(num(&s, "vmodel") as u32, scripthook_sdk::joaat("adder"));
    assert_eq!(num(&s, "in_vehicle"), 201.0, "spawned into it ('Spawn into the vehicle' is on by default)");
    assert_eq!(num(&s, "stat_sets"), 1.0);
    assert_eq!(num(&s, "armour"), 77.0, "memory cheat (freeze) on the title's armour field");
    assert!(log.contains("money: 17345"), "12345 + 5000 through the cash stat's out parameter\n{log}");
    let vp = vec3(&s, "vpos");
    assert!((vp[0] - 10.0).abs() < 0.01 && (vp[1] - 25.0).abs() < 0.01, "spawned 5 m ahead of the teleported player: {vp:?}");
    let p = vec3(&s, "pos");
    assert!((p[0] - 10.0).abs() < 0.01 && p[1] > 20.5, "teleported to Pier (10, 20, 30), then flew forward (+Y): {p:?}\n{log}");
    let unload = line(&log, "trainer: unloaded after");
    assert!(unload.contains("(0 failed), 0 errors"), "{unload}");
    assert!(report.contains("\"mods\""), "report.json");
    for n in ["test-title-mirror-world.png", "test-title-mirror-player.png", "test-title-mirror-about.png"] {
        let png = std::fs::read(shots.join(n)).unwrap_or_else(|_| panic!("{n} missing\n{log}"));
        assert_eq!(&png[..4], b"\x89PNG");
    }
    // the state was saved
    let st = std::fs::read_to_string(rig.mods.join("scripthook-trainer").join("state.toml")).unwrap();
    assert!(st.contains("health = true") && st.contains("fly = true"), "{st}");
}

#[test]
fn trainer_alone_in_the_present_time_fallback() {
    let Some(rig) = rig("fallback") else {
        eprintln!("srt.exe or Sonoma Runtime's test title not found: skipping");
        return;
    };
    let rel = build_rust_plugins();
    let r = repo();
    rig.add_mod("scripthook-trainer", &rel.join("shk_trainer.dll"), &r.join("trainer/mod.toml"), &[]);
    rig.trainer_data(false);
    let auto = rig.base.join("trainer-script.txt");
    write(&auto, "wait 5\nfeature set_hour value 7\nfeature weather pick 0\nwait 40\nlog end\n");
    let (code, log, _) = rig.run(150, &[("SRT_TRAINER_DATA", rig.trainer_data.display().to_string()), ("SHK_TRAINER_SCRIPT", auto.display().to_string())]);
    std::fs::write(rig.base.join("srt.log"), &log).unwrap();
    assert_eq!(code, 0, "{log}");
    assert!(log.contains("scripts: present-time fallback"), "{log}");
    assert!(log.contains("trainer: pages in the host's basic menu"), "{log}");
    assert!(log.contains("trainer script: done, 0 failure(s)"), "{log}");
    let s = summary(&log);
    assert_eq!(num(&s, "hour"), 7.0);
    assert_eq!(s["weather"], "CLEAR");
}

/// Optional: the hook DLL itself loaded as a Sonoma Runtime mod (the runtime's plugin loading is
/// only used to get the DLL into the process; the hook uses none of its API). It finds the test
/// title's image mapped by the runtime's own loader, its natives table and script tick by
/// pattern / export, and hooks the runtime's DXGI Present.
#[test]
fn hook_dll_as_a_sonoma_runtime_mod() {
    let Some(rig) = rig("srmod") else {
        eprintln!("srt.exe or Sonoma Runtime's test title not found: skipping");
        return;
    };
    let rel = build_rust_plugins();
    let o = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())).current_dir(repo()).args(["build", "--release", "-p", "shk-core"]).output().unwrap();
    assert!(o.status.success());
    let r = repo();
    let m = rig.mods.join("gta-vi-scripthook");
    let cp = |a: &std::path::Path, b: std::path::PathBuf| {
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        std::fs::copy(a, b).unwrap();
    };
    cp(&rel.join("scripthook.dll"), m.join("scripthook.dll"));
    cp(&r.join("core/mod.toml"), m.join("mod.toml"));
    cp(&r.join("natives/examples/test-title.tsv"), m.join("natives.tsv"));
    write(&m.join("scripthook.toml"), "game_module = \"srt_test_title.exe\"\ntick_wait = 10\n");
    cp(&rel.join("hello_rust.dll"), m.join("plugins/hello-rust/hello_rust.dll"));
    cp(&rel.join("shk_trainer.dll"), m.join("plugins/trainer/shk_trainer.dll"));
    write(&m.join("plugins/trainer/plugin.toml"), "plugin = \"shk_trainer.dll\"\n");
    write(&m.join("plugins/trainer/state.toml"), "[on]\nhealth = true\nshow_fps = true\n");
    let auto = rig.base.join("trainer-script.txt");
    write(&auto, "wait 20\nfeature set_hour value 9\nwait 20\nlog end\n");
    let shot = rig.base.join("sr-overlay.png");
    let cap = rig.base.join("automation.txt");
    write(&cap, format!("wait 120\ncapture {}\nwait 3\nstats\n", shot.display()));
    let (code, srlog, _) = rig.run(240, &[("SHK_TRAINER_SCRIPT", auto.display().to_string()), ("SHK_AUTOMATION", cap.display().to_string())]);
    let log = std::fs::read_to_string(m.join("scripthook.log")).unwrap_or_default();
    std::fs::write(rig.base.join("srt.log"), &srlog).unwrap();
    assert_eq!(code, 0, "{srlog}\n{log}");
    assert!(log.contains("scripts: running on the game's script thread"), "{log}");
    assert!(log.contains("overlay: hooked IDXGISwapChain::Present"), "{log}");
    assert!(log.contains("hello-rust: native PLAYER_PED_ID -> ped 100 at ("), "{log}");
    assert!(log.contains("trainer script: done, 0 failure(s)"), "{log}");
    let s = summary(&srlog);
    assert_eq!(num(&s, "invincible"), 1.0, "{log}");
    assert_eq!(num(&s, "hour"), 9.0, "{log}");
    let png = std::fs::read(&shot).unwrap_or_else(|_| panic!("no capture\n{log}"));
    assert!(scripthook_sdk::png::decode_own(&png).is_some());
    let keep = repo().join("target").join("shk-shots");
    let _ = std::fs::create_dir_all(&keep);
    let _ = std::fs::copy(&shot, keep.join("sonoma-runtime-overlay.png"));
}
