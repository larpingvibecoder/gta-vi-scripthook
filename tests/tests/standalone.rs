//! The hook on its own: `shk-loader` starts the synthetic test target and loads `scripthook.dll`,
//! which finds the target's natives table and script tick by pattern, hooks the tick and the
//! target's DXGI Present, loads the menu, the trainer and the examples from `plugins\`, and draws
//! the menu with its own D3D12 renderer. No Sonoma Runtime involved.

use shk_tests::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("{} -> {}: {e}", from.display(), to.display()));
}

fn build() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let o = Command::new(cargo)
        .current_dir(repo())
        .env("CARGO_BUILD_JOBS", std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "6".into()))
        .args(["build", "--release", "-p", "shk-core", "-p", "shk-loader", "-p", "shk-menu", "-p", "shk-trainer", "-p", "hello-rust"])
        .output()
        .expect("cargo");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    repo().join("target").join("release")
}

fn line<'a>(log: &'a str, needle: &str) -> &'a str {
    log.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {needle:?} in\n{log}"))
}

fn fields(s: &str) -> std::collections::HashMap<String, String> {
    s.split_whitespace().filter_map(|w| w.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn hook_menu_trainer_and_examples_on_the_test_target() {
    let rel = build();
    let base = scratch("standalone");
    let game = base.join("game");
    let hook = base.join("hook");
    copy(Path::new(env!("CARGO_BIN_EXE_shk-test-target")), &game.join("shk-test-target.exe"));
    copy(&rel.join("scripthook.dll"), &hook.join("scripthook.dll"));
    copy(&rel.join("shk-loader.exe"), &hook.join("shk-loader.exe"));
    write(&hook.join("scripthook.toml"), "game_module = \"shk-test-target.exe\"\ntick_wait = 10\n");
    write(&hook.join("natives.tsv"), target::tsv(true));
    let r = repo();
    let plugins = hook.join("plugins");
    copy(&rel.join("shk_menu.dll"), &plugins.join("menu").join("shk_menu.dll"));
    copy(&r.join("menu").join("menu.toml"), &plugins.join("menu").join("menu.toml"));
    write(&plugins.join("menu").join("plugin.toml"), "plugin = \"shk_menu.dll\"\n");
    let tdir = plugins.join("trainer");
    copy(&rel.join("shk_trainer.dll"), &tdir.join("shk_trainer.dll"));
    write(&tdir.join("plugin.toml"), "plugin = \"shk_trainer.dll\"\n");
    write(&tdir.join("trainer.toml"), "data = [\"data\"]\n");
    write(&tdir.join("state.toml"), "[on]\nhealth = true\nshow_fps = true\n");
    let data = tdir.join("data");
    let j = scripthook_sdk::joaat;
    write(&data.join("vehicles.txt"), format!("# model\thash\nadder\t{:#010X}\nbus\t{:#010X}\n", j("adder"), j("bus")));
    write(&data.join("weather.txt"), "CLEAR\t0x00000001\nRAIN\t0x00000002\n");
    write(&data.join("locations.tsv"), "name\tx\ty\tz\theading\nPier\t10\t20\t30\t90\n");
    write(
        &data.join("cheats.toml"),
        format!(
            "[[location]]\nid = \"test_armour\"\ntype = \"f32\"\nmodule = \"game\"\nsection = \".data\"\npattern = \"{}\"\noffset = 24\n\n[[cheat]]\nid = \"test_armour\"\nname = \"Armour 77 (test target)\"\ngroup = \"Player\"\nkind = \"freeze\"\ntype = \"f32\"\nvalue = 77\nlocation = \"test_armour\"\n",
            target::hex(target::PLAYER_MAGIC)
        ),
    );
    copy(&rel.join("hello_rust.dll"), &plugins.join("hello-rust").join("hello_rust.dll"));
    let c = c_build_dir();
    if let Some(c) = &c {
        copy(&c.join("hello_c.dll"), &plugins.join("hello-c").join("hello_c.dll"));
        copy(&c.join("hello_cpp.dll"), &plugins.join("hello-cpp").join("hello_cpp.dll"));
    }
    let trainer_script = base.join("trainer-script.txt");
    write(
        &trainer_script,
        "wait 10\nfeature health expect on\nfeature wanted_level value 3\nfeature add_money value 5000\nfeature add_money run\nfeature weather pick RAIN\nfeature set_hour value 18\n\
         feature tp_location pick Pier\nwait 20\nfeature spawn_vehicle text adder\nwait 30\ncheat test_armour on\nfeature heal run\nwait 140\nfeature fly on\nwait 60\nlog end\n",
    );
    let shot = base.join("menu.png");
    let auto = base.join("automation.txt");
    // hotkeys of the examples, then the menu: Trainer > World > Set hour +1, a capture, close
    write(
        &auto,
        format!("wait 90\nkey F7\nkey F8\nkey F9\nwait 20\nkey F4\nwait 3\nkey ENTER\nwait 2\nkey DOWN\nkey DOWN\nkey DOWN\nkey DOWN\nkey DOWN\nwait 2\nkey ENTER\nwait 2\nkey RIGHT\nwait 5\ncapture {}\nwait 5\nkey F4\nwait 10\nstats\n", shot.display()),
    );
    let summary = base.join("summary.txt");
    let o = Command::new(hook.join("shk-loader.exe"))
        .args(["--launch"])
        .arg(game.join("shk-test-target.exe"))
        .arg("--wait")
        .arg("--")
        .args(["--frames", "420", "--summary"])
        .arg(&summary)
        .env("SHK_AUTOMATION", &auto)
        .env("SHK_TRAINER_SCRIPT", &trainer_script)
        .env("SHK_INPUT", "synthetic:0,0,1")
        .output()
        .unwrap();
    let log = std::fs::read_to_string(hook.join("scripthook.log")).unwrap_or_default();
    std::fs::write(base.join("loader-output.txt"), [o.stdout.clone(), o.stderr.clone()].concat()).unwrap();
    assert!(o.status.success(), "loader: {}{}\n{log}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let sum = std::fs::read_to_string(&summary).unwrap_or_else(|_| panic!("no summary\n{log}"));
    let s = fields(&sum);
    // the hook found everything by itself
    assert!(log.contains("overlay: hooked IDXGISwapChain::Present"), "{log}");
    assert!(log.contains("scripts: running on the game's script thread"), "{log}");
    assert!(line(&log, "natives: ").contains(&format!("{} in the database", target::NATIVES.len())));
    for p in ["menu", "trainer", "hello-rust"] {
        assert!(log.contains(&format!("plugin {p} loaded")), "{p}\n{log}");
    }
    assert!(log.contains("menu: drawing its own UI"), "{log}");
    assert!(log.contains("trainer: pages in the scripthook menu service"), "{log}");
    // the examples: natives on the script thread, then their hotkeys
    let mut langs = vec![("rust", "F9")];
    if c.is_some() {
        langs.extend([("c", "F7"), ("cpp", "F8")]);
    }
    for (lang, key) in langs {
        assert!(line(&log, &format!("hello-{lang}: native PLAYER_PED_ID ->")).contains("100 at ("), "{log}");
        assert!(log.contains(&format!("hello-{lang}: {key} -> ped 100 at (")), "{lang}\n{log}");
    }
    // the trainer through the hook's natives
    assert!(log.contains("trainer script: done, 1 failure(s)"), "{log}");
    assert!(line(&log, "trainer script: feature heal run").contains("FAILED: needs native ENTITY::SET_ENTITY_HEALTH"), "{log}");
    assert!(log.contains("money: 17345"), "{log}");
    assert_eq!(s["hooked"], "1", "{sum}");
    assert_eq!(s["invincible"], "1", "{sum}");
    assert_eq!(s["health"], "100.00", "{sum}");
    assert_eq!(s["wanted"], "3", "{sum}");
    assert_eq!(s["hour"], "19", "18 from the automation, +1 from the menu's slider\n{sum}\n{log}");
    assert_eq!(s["weather"], "RAIN", "{sum}");
    assert_eq!(s["vehicles"], "1", "{sum}");
    assert_eq!(s["vmodel"].parse::<u32>().unwrap(), j("adder"), "{sum}");
    assert_eq!(s["in_vehicle"], "201", "{sum}");
    assert_eq!(s["stat_sets"], "1", "{sum}");
    assert_eq!(s["armour"], "77.00", "memory cheat\n{sum}");
    let pos: Vec<f64> = s["pos"].trim_matches(['(', ')']).split(',').map(|x| x.parse().unwrap()).collect();
    assert!((pos[0] - 10.0).abs() < 0.01 && pos[1] > 20.5, "teleported to Pier, then flew forward: {pos:?}\n{log}");
    assert!(log.contains("(0 failed), 0 errors"), "{log}");
    // the menu, drawn by the hook's renderer, in the captured frame
    let png = std::fs::read(&shot).unwrap_or_else(|_| panic!("no capture\n{log}"));
    let (w, h, px) = scripthook_sdk::png::decode_own(&png).expect("own PNG");
    assert_eq!((w, h), (1280, 720));
    let at = |x: u32, y: u32| -> [u8; 3] {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let header = at(300, 40);
    assert!(header[0] < 40 && header[1] < 40 && header[2] < 50, "dark header bar at the top left: {header:?}");
    let strip = (60..70).map(|y| at(300, y)).find(|p| p[1] > 150 && p[2] > 140 && p[0] < 90);
    assert!(strip.is_some(), "teal accent strip under the header");
    let bg = at(1000, 400);
    assert!(bg[0] > 60 && bg[1] < 40, "the target's own picture elsewhere: {bg:?}");
    let keep = repo().join("target").join("shk-shots");
    let _ = std::fs::create_dir_all(&keep);
    let _ = std::fs::copy(&shot, keep.join("test-target-menu.png"));
}
