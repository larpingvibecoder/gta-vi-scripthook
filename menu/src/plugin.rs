//! The plugin: publishes the menu service; draws the menu where the host can draw, mirrors it
//! into the host's menu otherwise. Configuration: `menu.toml` next to the DLL.

use crate::core::{MenuCore, Settings};
use crate::input::{Bindings, Frame, Input};
use crate::service::{self, Backend};
use scripthook_sdk::config::toml;
use scripthook_sdk::ffi::*;
use scripthook_sdk::menu::BasicMenu;
use scripthook_sdk::{self as shk, keys, Host};
use std::ffi::c_void;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Config {
    pub bindings: Bindings,
    pub settings: Settings,
    /// "auto" (draw when the host can) or "mirror" (always the host's menu)
    pub mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { bindings: Bindings::default(), settings: Settings::default(), mode: "auto".into() }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let v = toml::parse(text)?;
        let mut c = Config::default();
        if let Some(k) = v.str_of("open_key") {
            c.bindings.open_key = keys::vk_from_name(&k).ok_or_else(|| format!("open_key: unknown key {k}"))?;
        }
        if let Some(k) = v.str_of("open_chord") {
            c.bindings.open_chord = if k.is_empty() { 0 } else { keys::pad_chord(&k).ok_or_else(|| format!("open_chord: unknown buttons {k}"))? };
        }
        if let Some(b) = v.bool_of("numpad") {
            c.bindings.numpad = b;
        }
        if let Some(p) = v.str_of("position") {
            c.settings.right_side = p.eq_ignore_ascii_case("right");
        }
        if let Some(s) = v.f64_of("scale") {
            c.settings.scale = s.clamp(0.0, 3.0) as f32;
        }
        if let Some(h) = v.bool_of("show_hud") {
            c.settings.show_hud = h;
        }
        if let Some(m) = v.str_of("mode") {
            c.mode = m;
        }
        Ok(c)
    }
}

static INPUT: Mutex<Option<Input>> = Mutex::new(None);
static KEYS: Mutex<Vec<ShkKeyEvent>> = Mutex::new(Vec::new());

pub struct MenuPlugin {
    drawn: bool,
}

fn notify(t: &str, s: f32) {
    shk::host().notify(t, s);
}

impl shk::Plugin for MenuPlugin {
    fn load(host: Host) -> Result<Self, String> {
        let path = host.plugin_folder().join("menu.toml");
        let cfg = match std::fs::read_to_string(&path) {
            Ok(t) => Config::parse(&t).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(_) => Config::default(),
        };
        let drawn = host.can_draw() && cfg.mode != "mirror";
        if drawn {
            let mut core = MenuCore::new(&host.name());
            core.apply_settings(cfg.settings.clone());
            core.open_key = keys::vk_name(cfg.bindings.open_key);
            service::set_backend(Some(Backend::Drawn(Box::new(core))), Some(notify));
            *INPUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Input::new(cfg.bindings.clone()));
            let events = host.on_key(|ev| {
                KEYS.lock().unwrap_or_else(|e| e.into_inner()).push(*ev);
                // consumed while the menu is open (or for the open key)
                service::with(|b| matches!(b, Backend::Drawn(m) if m.nav.open)).unwrap_or(false)
            });
            host.on_frame(move |f| frame(f, events));
            host.on_draw(1, |c| {
                service::with(|b| {
                    if let Backend::Drawn(m) = b {
                        m.draw(c, service::now());
                    }
                });
            });
            host.info(&format!("menu: drawing its own UI ({} to open; input events: {})", keys::vk_name(cfg.bindings.open_key), if events { "yes" } else { "polled" }));
        } else {
            let api = host.basic_menu().ok_or("the host can neither draw for plugins nor show a menu")?;
            let mut bm = BasicMenu::new(api, shk::rt::self_ptr(), "");
            let about = bm.add_page(0, "gta-vi-scripthook");
            for line in [
                format!("gta-vi-scripthook menu v{}", env!("CARGO_PKG_VERSION")),
                "Mirror mode: this host cannot draw for plugins yet,".to_string(),
                "so plugin pages are listed in its own menu.".to_string(),
                "Unofficial; single-player / offline only.".to_string(),
            ] {
                bm.add_item(0, about, &scripthook_sdk::menu::ItemSpec::text(line), None, std::ptr::null_mut());
            }
            service::set_backend(Some(Backend::Mirror(bm)), Some(notify));
            host.info("menu: mirror mode (the host has no drawing primitives for plugins; pages go to its menu)");
        }
        if !host.publish(SHK_MENU_SERVICE, &service::SERVICE as *const ShkMenuService as *const c_void) {
            return Err("could not publish the menu service (another menu plugin is loaded?)".into());
        }
        host.info(&format!("menu: service {SHK_MENU_SERVICE} published on {}", host.name()));
        Ok(MenuPlugin { drawn })
    }

    fn unload(&mut self) {
        let h = shk::host();
        h.withdraw(SHK_MENU_SERVICE);
        if self.drawn {
            h.capture_input(false);
        }
        service::set_backend(None, None);
    }
}

fn frame(f: &ShkFrameInfo, events: bool) {
    let h = shk::host();
    let now = service::now();
    let (open, editing) = service::with(|b| match b {
        Backend::Drawn(m) => (m.nav.open, m.nav.editing()),
        _ => (false, false),
    })
    .unwrap_or((false, false));
    let mut out = Frame::default();
    {
        let mut g = INPUT.lock().unwrap_or_else(|e| e.into_inner());
        let Some(inp) = g.as_mut() else { return };
        if f.menu_open == 0 {
            if events {
                for ev in std::mem::take(&mut *KEYS.lock().unwrap_or_else(|e| e.into_inner())) {
                    inp.key_event(&ev, open, editing, &mut out);
                }
            } else {
                inp.poll_keys(&|vk| h.key_down(vk), open, editing, now, &mut out);
            }
            inp.poll_pad(h.pad(0), open, now, &mut out);
        }
    }
    let d = service::with(|b| match b {
        Backend::Drawn(m) => {
            if out.toggle {
                let o = !m.nav.open;
                m.set_open(o);
            }
            m.handle(&out.navs)
        }
        _ => Vec::new(),
    })
    .unwrap_or_default();
    if out.toggle {
        h.capture_input(!open);
    }
    service::run(d);
}

shk::export_plugin!(MenuPlugin, name = "gta-vi-scripthook menu", version = "0.1.0", author = "The gta-vi-scripthook contributors", description = "The mod menu: the scripthook.menu service, drawn by itself or mirrored into the host's menu.");

#[cfg(test)]
mod tests {
    #[test]
    fn config() {
        let c = super::Config::parse("open_key = \"F10\"\nopen_chord = \"LB+RB\"\nposition = \"right\"\nscale = 2\nmode = \"mirror\"\n").unwrap();
        assert_eq!(c.bindings.open_key, 0x79);
        assert!(c.settings.right_side);
        assert_eq!(c.settings.scale, 2.0);
        assert_eq!(c.mode, "mirror");
        assert!(super::Config::parse("open_key = \"NOPE\"").is_err());
    }
}
