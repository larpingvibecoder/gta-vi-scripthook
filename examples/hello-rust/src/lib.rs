//! hello-rust: the smallest gta-vi-scripthook plugin in Rust.
//!
//! A notice (and a drawn box where the host can draw), typed native calls from a script
//! function, and a hotkey (F9). `cargo build --release -p hello-rust` gives
//! `target\release\hello_rust.dll`; the SDK exports both entry points (scripthook.h and Sonoma
//! Runtime's plugin API version 2).

use scripthook_sdk::draw::{argb, Painter};
use scripthook_sdk::natives::{invoke, Vec3};
use scripthook_sdk::{self as shk, keys, Host};
use std::sync::atomic::{AtomicBool, Ordering};

static ASKED: AtomicBool = AtomicBool::new(false);

struct Hello;

fn report(what: &str) -> Result<(), shk::natives::NativeError> {
    let ped: i32 = invoke("PLAYER::PLAYER_PED_ID", ())?;
    let p: Vec3 = invoke("ENTITY::GET_ENTITY_COORDS", (ped, true))?;
    let t = format!("hello-rust: {what} -> ped {ped} at ({:.2}, {:.2}, {:.2})", p.x, p.y, p.z);
    shk::host().info(&t);
    if what == "F9" {
        shk::host().notify(&t, 3.0);
    }
    Ok(())
}

impl shk::Plugin for Hello {
    fn load(host: Host) -> Result<Self, String> {
        host.info(&format!("hello-rust: loaded on {}", host.name()));
        host.notify("Hello from Rust (F9: where am I?)", 4.0);
        host.on_draw(0, |c| {
            let s = c.ui_scale();
            let (x, y) = (c.size().0 as f32 - 132.0 * s, 104.0 * s);
            c.rect(x, y, 120.0 * s, 18.0 * s, argb(220, 12, 16, 22));
            c.rect(x, y, 2.0 * s, 18.0 * s, argb(255, 46, 196, 182));
            c.text(x + 8.0 * s, y + 6.0 * s, s, argb(255, 232, 237, 242), "Hello from Rust");
        });
        host.hotkey(keys::vk_from_name("F9").unwrap(), 0, || ASKED.store(true, Ordering::Relaxed));
        host.start_script("hello-rust", || {
            if let Err(e) = report("native PLAYER_PED_ID") {
                shk::host().warn(&format!("hello-rust: {e}"));
                return;
            }
            loop {
                if ASKED.swap(false, Ordering::Relaxed) {
                    let _ = report("F9");
                }
                if !shk::wait(0) {
                    return;
                }
            }
        });
        Ok(Hello)
    }

    fn unload(&mut self) {
        shk::host().info("hello-rust: unloaded");
    }
}

shk::export_plugin!(Hello, name = "hello-rust", version = "0.1.0", author = "The gta-vi-scripthook contributors", description = "Hello world in Rust: a notice, a native call, a hotkey.");
