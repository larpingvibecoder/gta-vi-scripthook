//! Renders previews of the menu with its software canvas (the host's font and metrics) to PNG
//! files: `menu-preview <out dir> [width height]`. The pages are a demo shaped like the
//! trainer's; the background is an abstract gradient (no game imagery).

use scripthook_sdk::draw::{argb, Painter, SoftCanvas};
use scripthook_sdk::menu::ItemSpec;
use shk_menu::core::MenuCore;
use shk_menu::model::ROOT;
use shk_menu::nav::Nav;
use std::path::PathBuf;

fn background(w: u32, h: u32) -> SoftCanvas {
    let mut c = SoftCanvas::new(w, h, 0);
    c.gradient(argb(255, 24, 38, 56), argb(255, 10, 14, 22));
    // a faint grid and a horizon band, to show the panel's translucency
    let grid = argb(18, 120, 200, 220);
    let mut x = 0.0;
    while x < w as f32 {
        c.rect(x, 0.0, 1.0, h as f32, grid);
        x += 48.0;
    }
    let mut y = 0.0;
    while y < h as f32 {
        c.rect(0.0, y, w as f32, 1.0, grid);
        y += 48.0;
    }
    c.rect(0.0, h as f32 * 0.62, w as f32, h as f32 * 0.02, argb(40, 46, 196, 182));
    c
}

fn demo() -> MenuCore {
    let mut m = MenuCore::new("scripthook.dll (preview)");
    let me = 1;
    let tr = m.model.add_page(me, ROOT, "Trainer", "44 features: natives and memory cheats, data-driven.");
    let tools = m.model.add_page(me, ROOT, "Script tools", "An example plugin page.");
    let groups = ["Player", "Teleport", "Vehicle", "Weapons", "World", "Misc"];
    let mut pages = Vec::new();
    m.model.add_item(me, tr, ItemSpec::text("Script hook: game script thread").right("33 natives").hint("Scripts run on the game's script thread."), None);
    for g in groups {
        let p = m.model.add_page(me, tr, g, "");
        pages.push(p);
        m.model.add_item(me, tr, ItemSpec::subpage(g, p), None);
    }
    let player = pages[0];
    m.model.add_item(me, player, ItemSpec::toggle("Invincible", true).hint("PLAYER::SET_PLAYER_INVINCIBLE every script tick.").hotkey(0x75), None);
    m.model.add_item(me, player, ItemSpec::button("Heal and refill armour").hint("Health to maximum, armour to 100, blood cleared."), None);
    m.model.add_item(me, player, ItemSpec::toggle("Unlimited armour", false), None);
    m.model.add_item(me, player, ItemSpec::slider("  Armour value", 100.0, 0.0, 200.0, 10.0), None);
    m.model.add_item(me, player, ItemSpec::toggle("Never wanted", true), None);
    m.model.add_item(me, player, ItemSpec::slider("Wanted level", 3.0, 0.0, 5.0, 1.0), None);
    m.model.add_item(me, player, ItemSpec::toggle("Super jump", false).disabled("needs native MISC::SET_SUPER_JUMP_THIS_FRAME (unknown until the executable)"), None);
    m.model.add_item(me, player, ItemSpec::toggle("Fast run", false), None);
    m.model.add_item(me, player, ItemSpec::number("Add money: amount", 100000.0, 1000.0), None);
    m.model.add_item(me, player, ItemSpec::button("Add money").hint("Adds the amount to the cash stat."), None);
    m.model.add_item(me, player, ItemSpec::toggle("Unlimited money", false).disabled("memory cheat: location unknown until the executable is available"), None);
    m.model.add_item(me, player, ItemSpec::subpage("Change player model", 0), None);
    let veh = m.model.add_page(me, pages[2], "Spawn vehicle", "Pick a model or type its name.");
    m.model.add_item(me, pages[2], ItemSpec::subpage("Spawn vehicle", veh).filterable(), None);
    m.model.add_item(me, pages[2], ItemSpec::toggle("Spawn into the vehicle", true), None);
    m.model.add_item(me, veh, ItemSpec::input("Type a model name", ""), None);
    for (i, n) in ["Model A (demo)", "Model B (demo)", "Model C (demo)", "Bus (demo)", "Bike (demo)", "Boat (demo)", "Van (demo)", "Truck (demo)", "Coupe (demo)", "Sedan (demo)", "Taxi (demo)", "Tractor (demo)", "Plane (demo)", "Heli (demo)"].iter().enumerate() {
        m.model.add_item(me, veh, ItemSpec::button(*n).right(format!("0x{:08X}", 0x1000_0000u32 + i as u32 * 0x1111)), None);
    }
    let world = pages[4];
    m.model.add_item(me, world, ItemSpec::slider("Set hour", 18.0, 0.0, 23.0, 1.0), None);
    m.model.add_item(me, world, ItemSpec::toggle("Freeze time", false), None);
    m.model.add_item(me, world, ItemSpec::choice("Weather", vec!["CLEAR".into(), "CLOUDS".into(), "RAIN".into(), "FOG".into()], 2), None);
    m.model.add_item(me, tools, ItemSpec::text("Plugins add pages like this one."), None);
    m.model.set_hud(me, 0, "pos (10.00, 25.00, 30.00)  heading 90");
    m.model.set_hud(me, 1, "FPS 60");
    m
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = PathBuf::from(args.first().cloned().unwrap_or_else(|| "docs/images".into()));
    let w: u32 = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(1280);
    let h: u32 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(720);
    std::fs::create_dir_all(&out).expect("output folder");
    let shots: Vec<(&str, Vec<Nav>, f64)> = vec![
        ("menu-home.png", vec![], 0.0),
        ("menu-player.png", vec![Nav::Accept, Nav::Down, Nav::Accept, Nav::Down, Nav::Down, Nav::Down], 0.0),
        ("menu-unavailable.png", vec![Nav::Accept, Nav::Down, Nav::Accept, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Down], 0.0),
        ("menu-vehicles.png", vec![Nav::Accept, Nav::Down, Nav::Down, Nav::Down, Nav::Accept, Nav::Accept, Nav::Char('b'), Nav::Down], 0.2),
        ("menu-world.png", vec![Nav::Accept, Nav::End, Nav::Up, Nav::Accept, Nav::Down, Nav::Down], 0.0),
    ];
    for (name, navs, t) in shots {
        let mut m = demo();
        m.set_open(true);
        if name == "menu-world.png" {
            m.model.toast("weather: RAIN", 3.0, 0.0);
        }
        for n in navs {
            let mut scratch = background(w, h);
            m.draw(&mut scratch, t); // layout (rows per page) as on screen
            m.handle(&[n]);
        }
        let mut c = background(w, h);
        m.draw(&mut c, t);
        let p = out.join(name);
        std::fs::write(&p, c.png()).expect("write png");
        println!("wrote {} ({}x{}, scale {})", p.display(), w, h, c.ui_scale());
    }
}
