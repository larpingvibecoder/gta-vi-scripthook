//! scripthook.h (compiled by MSVC) and the Rust SDK agree on every size and offset; the example
//! natives database parses, and its generated headers compiled (tests/build.rs).

use scripthook_sdk::ffi::*;
use std::collections::HashMap;
use std::mem::{offset_of, size_of};

#[test]
fn c_header_and_rust_sdk_agree() {
    let Some(dir) = shk_tests::c_build_dir() else {
        eprintln!("MSVC not found: skipping the C side");
        return;
    };
    let o = std::process::Command::new(dir.join("abi_check.exe")).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let c: HashMap<String, usize> = text.lines().filter_map(|l| {
        let mut w = l.split_whitespace();
        let (_, k, v) = (w.next()?, w.next()?, w.next()?);
        Some((k.to_string(), v.parse().ok()?))
    }).collect();
    let rust: Vec<(&str, usize)> = vec![
        ("ShkVec3", size_of::<ShkVec3>()),
        ("ShkRegion", size_of::<ShkRegion>()),
        ("ShkModule", size_of::<ShkModule>()),
        ("ShkFrameInfo", size_of::<ShkFrameInfo>()),
        ("ShkPad", size_of::<ShkPad>()),
        ("ShkKeyEvent", size_of::<ShkKeyEvent>()),
        ("ShkValue", size_of::<ShkValue>()),
        ("ShkNativeInfo", size_of::<ShkNativeInfo>()),
        ("ShkScriptStatus", size_of::<ShkScriptStatus>()),
        ("ShkCanvas", size_of::<ShkCanvas>()),
        ("ShkPluginInfo", size_of::<ShkPluginInfo>()),
        ("ShkMenuEvent", size_of::<ShkMenuEvent>()),
        ("ShkMenuItemDesc", size_of::<ShkMenuItemDesc>()),
        ("ShkCoreApi", size_of::<ShkCoreApi>()),
        ("ShkMemoryApi", size_of::<ShkMemoryApi>()),
        ("ShkHookApi", size_of::<ShkHookApi>()),
        ("ShkInputApi", size_of::<ShkInputApi>()),
        ("ShkNativeApi", size_of::<ShkNativeApi>()),
        ("ShkScriptApi", size_of::<ShkScriptApi>()),
        ("ShkHostMenuApi", size_of::<ShkHostMenuApi>()),
        ("ShkDrawApi", size_of::<ShkDrawApi>()),
        ("ShkServiceApi", size_of::<ShkServiceApi>()),
        ("ShkHost", size_of::<ShkHost>()),
        ("ShkMenuService", size_of::<ShkMenuService>()),
        ("ShkValue.u", offset_of!(ShkValue, u)),
        ("ShkNativeInfo.ret_type", offset_of!(ShkNativeInfo, ret_type)),
        ("ShkNativeInfo.name", offset_of!(ShkNativeInfo, name)),
        ("ShkNativeInfo.reason", offset_of!(ShkNativeInfo, reason)),
        ("ShkScriptStatus.reason", offset_of!(ShkScriptStatus, reason)),
        ("ShkCanvas.ctx", offset_of!(ShkCanvas, ctx)),
        ("ShkCanvas.clip", offset_of!(ShkCanvas, clip)),
        ("ShkInputApi.on_key", offset_of!(ShkInputApi, on_key)),
        ("ShkInputApi.capture", offset_of!(ShkInputApi, capture)),
        ("ShkHost.services", offset_of!(ShkHost, services)),
        ("ShkHost.draw", offset_of!(ShkHost, draw)),
        ("ShkMenuItemDesc.value", offset_of!(ShkMenuItemDesc, value)),
        ("ShkMenuItemDesc.target", offset_of!(ShkMenuItemDesc, target)),
        ("ShkMenuItemDesc.on_event", offset_of!(ShkMenuItemDesc, on_event)),
        ("ShkMenuItemDesc.user", offset_of!(ShkMenuItemDesc, user)),
        ("ShkMenuService.toast", offset_of!(ShkMenuService, toast)),
        ("ShkMenuEvent.value", offset_of!(ShkMenuEvent, value)),
        ("ShkMenuEvent.text", offset_of!(ShkMenuEvent, text)),
    ];
    assert_eq!(c.len(), rust.len(), "{text}");
    for (k, v) in rust {
        assert_eq!(c.get(k), Some(&v), "{k}: C {:?}, Rust {v}", c.get(k));
    }
    // the structures shared with Sonoma Runtime's plugin API version 2 keep its sizes
    for (k, v) in [("ShkValue", 24), ("ShkNativeInfo", 408), ("ShkScriptStatus", 184), ("ShkRegion", 40), ("ShkModule", 88), ("ShkFrameInfo", 32), ("ShkPad", 28)] {
        assert_eq!(c[k], v, "{k} must stay layout-compatible with plugin API v2");
    }
}

#[test]
fn example_database_is_valid_and_in_sync() {
    let t = std::fs::read_to_string(shk_tests::repo().join("natives/examples/test-title.tsv")).unwrap();
    let mut d = shk_natives::db::parse(&t);
    d.check();
    assert!(d.errors.is_empty(), "{:?}", d.errors);
    assert_eq!(d.natives.len(), 33);
    assert_eq!(d.directives.len(), 3);
    // identifiers follow the test title's rule
    for n in &d.natives {
        let j = scripthook_sdk::joaat(&n.name) as u64;
        assert_eq!(n.id, Some(shk_natives::db::Id::Hash(j << 32 | (j ^ 0x9E37_79B9))), "{}", n.name);
    }
    // the verify tool finds the table and the tick in the test title
    if let Some(dir) = shk_tests::title_dir() {
        let pe = shk_natives::pe::Pe::parse(std::fs::read(dir.join("srt_test_title.exe")).unwrap()).unwrap();
        let checks = shk_natives::verify::verify(&d, &pe, "srt_test_title.exe");
        assert_eq!(checks.len(), 2);
        for c in checks {
            assert!(matches!(c.outcome, shk_natives::verify::Outcome::Found(_)), "@{}: {:?}", c.directive, c.outcome);
        }
    }
}
